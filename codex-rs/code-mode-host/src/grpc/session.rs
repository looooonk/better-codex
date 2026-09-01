use std::borrow::Borrow;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::collections::hash_map::Entry;
use std::future::Future;
use std::hash::Hash;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::Weak;

use codex_code_mode_protocol::CellId;
use codex_code_mode_protocol::CodeModeSessionCellExecutionLimits;
use codex_code_mode_protocol::WaitOutcome;
use codex_code_mode_protocol::grpc as proto;
use codex_code_mode_protocol::grpc::MAX_APPLICATION_MESSAGE_BYTES;
use codex_code_mode::InProcessCodeModeSession;
use serde_json::Value as JsonValue;
use tokio::sync::Notify;
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::Semaphore;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tonic::Status;
use uuid::Uuid;

use super::GrpcStream;
use super::delegate::GrpcDelegate;
use super::events::EventSender;
use super::events::MAX_HOST_EVENT_BYTES;
use super::events::MAX_SESSION_EVENT_BYTES;
use super::principal::GrpcPrincipal;
use super::validation;
use super::waits::ActiveWait;
use crate::HostLimits;
use crate::MAX_ACTIVE_CELLS;
use crate::MAX_IN_FLIGHT_REQUESTS;
use crate::MAX_RECENT_REQUEST_IDS;
use crate::OUTGOING_CHANNEL_CAPACITY;

pub(crate) const MAX_OPEN_GRPC_SESSIONS: usize = 6;
pub(crate) const MAX_HOST_TOOL_BYTES: usize = MAX_APPLICATION_MESSAGE_BYTES * 8;
const MAX_SESSION_TOOL_BYTES: usize = MAX_APPLICATION_MESSAGE_BYTES * 2;
pub(crate) const MAX_GRPC_PENDING_DELEGATE_CALLS: usize = 8;

pub(super) struct GrpcHostState {
    sessions: Mutex<HashMap<Uuid, Arc<GrpcSession>>>,
    limits: HostLimits,
    delegate_permits: Arc<Semaphore>,
    control_permits: Arc<Semaphore>,
    event_byte_permits: Arc<Semaphore>,
    tool_byte_permits: Arc<Semaphore>,
}

pub(super) struct GrpcSession {
    pub(super) id: Uuid,
    principal: GrpcPrincipal,
    pub(super) runtime: Arc<InProcessCodeModeSession>,
    pub(super) closed: CancellationToken,
    pub(super) state: Mutex<SessionState>,
    events: EventSender,
    cells_changed: Notify,
    delegate_permits: Arc<Semaphore>,
    session_tool_byte_permits: Arc<Semaphore>,
    host_tool_byte_permits: Arc<Semaphore>,
    tasks: TaskTracker,
}

#[derive(Default)]
pub(super) struct SessionState {
    shutdown_started: bool,
    pub(super) cells: HashMap<String, ExecutionState>,
    pending_executions: HashSet<String>,
    pending_closures: HashSet<String>,
    seen_executions: BoundedIds,
    pub(super) subscriptions: Vec<ToolSubscription>,
    pub(super) next_subscription: usize,
    pub(super) pending_invocations: HashMap<Uuid, PendingInvocation>,
    pub(super) seen_invocations: BoundedIds<Uuid>,
    pub(super) pending_notifications: HashMap<Uuid, oneshot::Sender<()>>,
    seen_notifications: BoundedIds<Uuid>,
    pub(super) waits: HashMap<String, ActiveWait>,
    pub(super) seen_waits: BoundedIds,
    pub(super) cancelled_waits: BoundedIds,
}

pub(super) struct ExecutionState {
    pub(super) execution_id: String,
    pub(super) tool_call_sequence: u64,
    pub(super) runtime_closed: bool,
    pub(super) terminal_observed: bool,
    permit: OwnedSemaphorePermit,
}

pub(super) struct ToolSubscription {
    pub(super) id: Uuid,
    pub(super) filters: Vec<proto::ToolName>,
    pub(super) sender: mpsc::Sender<BufferedToolCall>,
}

pub(super) struct BufferedToolCall {
    pub(super) message: proto::ToolCall,
    _reservation: ToolByteReservation,
}

pub(super) struct ToolByteReservation {
    _session: OwnedSemaphorePermit,
    _host: OwnedSemaphorePermit,
}

impl ToolByteReservation {
    pub(super) fn retain(mut self, bytes: usize) -> Result<Self, String> {
        let session = self
            ._session
            .split(bytes)
            .ok_or_else(|| "code-mode session tool-call reservation was too small".to_string())?;
        let host = self
            ._host
            .split(bytes)
            .ok_or_else(|| "code-mode host tool-call reservation was too small".to_string())?;
        Ok(Self {
            _session: session,
            _host: host,
        })
    }
}

pub(super) struct PendingInvocation {
    pub(super) subscription_id: Uuid,
    pub(super) response: oneshot::Sender<Result<JsonValue, String>>,
}

#[derive(Default)]
pub(super) struct BoundedIds<T = String> {
    ids: HashSet<T>,
    order: VecDeque<T>,
}

impl GrpcHostState {
    pub(super) fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            limits: HostLimits::new(),
            delegate_permits: Arc::new(Semaphore::new(MAX_GRPC_PENDING_DELEGATE_CALLS)),
            control_permits: Arc::new(Semaphore::new(MAX_IN_FLIGHT_REQUESTS)),
            event_byte_permits: Arc::new(Semaphore::new(MAX_HOST_EVENT_BYTES)),
            tool_byte_permits: Arc::new(Semaphore::new(MAX_HOST_TOOL_BYTES)),
        }
    }

    pub(super) fn open_session(
        self: &Arc<Self>,
        limits: CodeModeSessionCellExecutionLimits,
        principal: GrpcPrincipal,
    ) -> Result<GrpcStream<proto::SessionEvent>, Status> {
        let mut sessions = self.sessions.lock().unwrap_or_else(PoisonError::into_inner);
        if sessions.len() >= MAX_OPEN_GRPC_SESSIONS {
            return Err(Status::resource_exhausted(
                "code-mode host has too many open sessions",
            ));
        }
        let id = Uuid::new_v4();
        let (events, receiver) = mpsc::channel(OUTGOING_CHANNEL_CAPACITY);
        let closed = CancellationToken::new();
        let event_sender = EventSender::new(
            events.clone(),
            closed.clone(),
            Arc::new(Semaphore::new(MAX_SESSION_EVENT_BYTES)),
            Arc::clone(&self.event_byte_permits),
        );
        let session = GrpcSession::new(
            id,
            principal,
            event_sender,
            closed,
            Arc::clone(&self.delegate_permits),
            Arc::clone(&self.tool_byte_permits),
            limits,
        );
        session
            .send_event_now(
                proto::session_event::Event::Opened(proto::SessionOpened {
                    session_id: id.to_string(),
                }),
                /*cell_permit*/ None,
            )
            .map_err(Status::internal)?;
        sessions.insert(id, Arc::clone(&session));
        drop(sessions);

        let host = Arc::downgrade(self);
        tokio::spawn(async move {
            tokio::select! {
                _ = events.closed() => {}
                _ = session.closed.cancelled() => {}
            }
            if let Some(host) = host.upgrade() {
                host.close_lease(id, &session).await;
            } else {
                let _ = session.shutdown().await;
            }
        });

        Ok(super::events::event_stream(receiver))
    }

    pub(super) fn session_for_principal(
        &self,
        id: &str,
        principal: GrpcPrincipal,
    ) -> Result<Arc<GrpcSession>, Status> {
        let session_id = validation::uuid(id, "session ID")?;
        let session = self
            .sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&session_id)
            .cloned()
            .ok_or_else(|| Status::not_found(format!("unknown code-mode session {id}")))?;
        if session.principal != principal {
            return Err(Status::permission_denied(
                "code-mode session belongs to another caller",
            ));
        }
        Ok(session)
    }

    #[cfg(test)]
    pub(super) fn session(&self, id: &str) -> Result<Arc<GrpcSession>, Status> {
        let session_id = validation::uuid(id, "session ID")?;
        self.sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&session_id)
            .cloned()
            .ok_or_else(|| Status::not_found(format!("unknown code-mode session {id}")))
    }

    pub(super) fn take_session_for_close(
        &self,
        id: &str,
        principal: GrpcPrincipal,
    ) -> Result<Arc<GrpcSession>, Status> {
        let session_id = validation::uuid(id, "session ID")?;
        let mut sessions = self.sessions.lock().unwrap_or_else(PoisonError::into_inner);
        let session = sessions
            .get(&session_id)
            .ok_or_else(|| Status::not_found(format!("unknown code-mode session {id}")))?;
        if session.principal != principal {
            return Err(Status::permission_denied(
                "code-mode session belongs to another caller",
            ));
        }
        Ok(sessions.remove(&session_id).expect("session was checked above"))
    }

    async fn close_lease(&self, id: Uuid, expected: &Arc<GrpcSession>) {
        let session = {
            let mut sessions = self.sessions.lock().unwrap_or_else(PoisonError::into_inner);
            if sessions
                .get(&id)
                .is_some_and(|session| Arc::ptr_eq(session, expected))
            {
                sessions.remove(&id)
            } else {
                None
            }
        };
        if let Some(session) = session {
            let _ = session.shutdown().await;
        }
    }

    pub(super) fn request_permit(&self) -> Result<OwnedSemaphorePermit, Status> {
        self.limits.request_permit().map_err(|_| {
            Status::resource_exhausted("code-mode host has too many in-flight requests")
        })
    }

    pub(super) fn cell_permit(&self) -> Result<OwnedSemaphorePermit, Status> {
        self.limits
            .cell_permit()
            .map_err(|_| Status::resource_exhausted("code-mode host has too many active cells"))
    }

    pub(super) fn control_permit(&self) -> Result<OwnedSemaphorePermit, Status> {
        Arc::clone(&self.control_permits)
            .try_acquire_owned()
            .map_err(|_| {
                Status::resource_exhausted("code-mode host has too many in-flight control requests")
            })
    }
}

impl GrpcSession {
    fn new(
        id: Uuid,
        principal: GrpcPrincipal,
        events: EventSender,
        closed: CancellationToken,
        delegate_permits: Arc<Semaphore>,
        host_tool_byte_permits: Arc<Semaphore>,
        limits: CodeModeSessionCellExecutionLimits,
    ) -> Arc<Self> {
        Arc::new_cyclic(|weak: &Weak<Self>| {
            let delegate = Arc::new(GrpcDelegate::new(weak.clone()));
            let failure_session = weak.clone();
            let failure_handler = Arc::new(move |reason: String| {
                if let Some(session) = failure_session.upgrade() {
                    tracing::warn!(session_id = %session.id, "code-mode host session failed: {reason}");
                    session.closed.cancel();
                }
            });
            Self {
                id,
                principal,
                runtime: Arc::new(
                    InProcessCodeModeSession::with_delegate_and_task_failure_handler(
                        delegate,
                        failure_handler,
                        limits,
                    ),
                ),
                closed,
                state: Mutex::new(SessionState::default()),
                events,
                cells_changed: Notify::new(),
                delegate_permits,
                session_tool_byte_permits: Arc::new(Semaphore::new(MAX_SESSION_TOOL_BYTES)),
                host_tool_byte_permits,
                tasks: TaskTracker::new(),
            }
        })
    }

    pub(super) async fn shutdown(&self) -> Result<(), Status> {
        self.shutdown_with_deadline(tokio::time::Instant::now() + crate::SHUTDOWN_TIMEOUT)
            .await
    }

    pub(super) async fn shutdown_with_deadline(
        &self,
        deadline: tokio::time::Instant,
    ) -> Result<(), Status> {
        self.closed.cancel();
        {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.shutdown_started {
                return Ok(());
            }
            state.shutdown_started = true;
            self.tasks.close();
            for wait in state.waits.values() {
                wait.cancellation.cancel();
            }
            state.pending_invocations.clear();
            state.pending_notifications.clear();
            state.subscriptions.clear();
        }
        let shutdown = async {
            let (runtime, (), ()) = tokio::join!(
                self.runtime.shutdown(),
                self.events.shutdown(),
                self.tasks.wait(),
            );
            runtime.map_err(Status::internal)
        };
        let result = tokio::time::timeout_at(deadline, shutdown)
            .await
            .unwrap_or_else(|_| {
                Err(Status::deadline_exceeded(
                    "timed out shutting down code-mode gRPC session",
                ))
            });
        *self.state.lock().unwrap_or_else(PoisonError::into_inner) = SessionState {
            shutdown_started: true,
            ..SessionState::default()
        };
        result
    }

    pub(super) fn spawn_task(
        &self,
        task: impl Future<Output = ()> + Send + 'static,
    ) -> bool {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.shutdown_started || self.closed.is_cancelled() {
            return false;
        }
        self.tasks.spawn(task);
        true
    }

    pub(super) async fn terminate(&self, cell_id: CellId) -> Result<WaitOutcome, Status> {
        tokio::select! {
            biased;
            _ = self.closed.cancelled() => {
                Err(Status::cancelled("code-mode session is closed"))
            }
            result = self.runtime.terminate(cell_id) => {
                result.map_err(Status::failed_precondition)
            }
        }
    }

    pub(super) fn reserve_execution(&self, execution_id: &str) -> Result<(), Status> {
        validation::identifier(execution_id, "execution ID")?;
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if self.closed.is_cancelled() {
            return Err(Status::cancelled("code-mode session is closed"));
        }
        if state.pending_executions.contains(execution_id)
            || state
                .cells
                .values()
                .any(|execution| execution.execution_id == execution_id)
            || !state.seen_executions.remember(execution_id.to_string())
        {
            return Err(Status::already_exists(format!(
                "code-mode execution ID `{execution_id}` was reused"
            )));
        }
        state.pending_executions.insert(execution_id.to_string());
        Ok(())
    }

    pub(super) fn admit_execution(
        &self,
        execution_id: String,
        cell_id: String,
        permit: OwnedSemaphorePermit,
    ) -> Result<(), Status> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if self.closed.is_cancelled() {
            return Err(Status::cancelled("code-mode session is closed"));
        }
        if !state.pending_executions.remove(&execution_id) {
            return Err(Status::cancelled("code-mode execution was abandoned"));
        }
        let runtime_closed = state.pending_closures.remove(&cell_id);
        let Entry::Vacant(entry) = state.cells.entry(cell_id.clone()) else {
            return Err(Status::internal(
                "code-mode runtime reused an active cell ID",
            ));
        };
        entry.insert(ExecutionState {
            execution_id,
            tool_call_sequence: 0,
            runtime_closed,
            terminal_observed: false,
            permit,
        });
        drop(state);
        self.cells_changed.notify_waiters();
        Ok(())
    }

    pub(super) fn abandon_execution(self: &Arc<Self>, execution_id: &str) {
        let (cell_id, closed_execution) = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.pending_executions.remove(execution_id);
            let cell_id = state
                .cells
                .iter()
                .find(|(_, execution)| execution.execution_id == execution_id)
                .map(|(cell_id, _)| cell_id.clone());
            let closed_execution = cell_id.as_ref().and_then(|cell_id| {
                let should_remove = state.cells.get_mut(cell_id).is_some_and(|execution| {
                    execution.terminal_observed = true;
                    execution.runtime_closed
                });
                should_remove.then(|| state.cells.remove(cell_id)).flatten()
            });
            (cell_id, closed_execution)
        };
        if let (Some(cell_id), Some(execution)) = (&cell_id, closed_execution) {
            self.send_cell_closed(cell_id, execution);
        }
        if let Some(cell_id) = cell_id {
            let session = Arc::clone(self);
            self.spawn_task(async move {
                let _ = session.terminate(CellId::new(cell_id)).await;
            });
        }
    }

    pub(super) async fn execution_id(
        &self,
        cell_id: &str,
        cancellation: &CancellationToken,
    ) -> Result<String, String> {
        loop {
            let changed = self.cells_changed.notified();
            if let Some(execution_id) = self
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .cells
                .get(cell_id)
                .map(|execution| execution.execution_id.clone())
            {
                return Ok(execution_id);
            }
            tokio::select! {
                _ = self.closed.cancelled() => {
                    return Err("code-mode session closed before cell admission".to_string());
                }
                _ = cancellation.cancelled() => {
                    return Err("code-mode callback was cancelled before cell admission".to_string());
                }
                _ = changed => {}
            }
        }
    }

    pub(super) fn close_cell(&self, cell_id: &str) {
        let execution = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            let can_queue_closure = state.pending_closures.len() < MAX_ACTIVE_CELLS;
            match state.cells.get_mut(cell_id) {
                Some(execution) => {
                    execution.runtime_closed = true;
                    let should_remove = execution.terminal_observed;
                    should_remove.then(|| state.cells.remove(cell_id)).flatten()
                }
                None if can_queue_closure => {
                    state.pending_closures.insert(cell_id.to_string());
                    None
                }
                None => {
                    self.closed.cancel();
                    None
                }
            }
        };
        if let Some(execution) = execution {
            self.send_cell_closed(cell_id, execution);
        }
    }

    pub(super) fn terminal_outcome_observed(&self, cell_id: &str) {
        let execution = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            let Some(execution) = state.cells.get_mut(cell_id) else {
                return;
            };
            execution.terminal_observed = true;
            let should_remove = execution.runtime_closed;
            should_remove.then(|| state.cells.remove(cell_id)).flatten()
        };
        if let Some(execution) = execution {
            self.send_cell_closed(cell_id, execution);
        }
    }

    fn send_cell_closed(&self, cell_id: &str, execution: ExecutionState) {
        let _ = self.send_event_now(
            proto::session_event::Event::CellClosed(proto::CellClosed {
                execution_id: execution.execution_id,
                cell_id: cell_id.to_string(),
                final_tool_call_sequence: execution.tool_call_sequence,
            }),
            Some(execution.permit),
        );
    }

    pub(super) fn delegate_permit(&self) -> Result<OwnedSemaphorePermit, String> {
        Arc::clone(&self.delegate_permits)
            .try_acquire_owned()
            .map_err(|_| "code-mode host has too many pending delegate calls".to_string())
    }

    pub(super) fn reserve_tool_bytes(
        &self,
        bytes: usize,
    ) -> Result<ToolByteReservation, String> {
        if bytes == 0 || bytes > MAX_APPLICATION_MESSAGE_BYTES {
            return Err("invalid code-mode tool-call byte reservation".to_string());
        }
        let bytes = u32::try_from(bytes)
            .map_err(|_| "code-mode tool-call budget exceeds this platform".to_string())?;
        let session = Arc::clone(&self.session_tool_byte_permits)
            .try_acquire_many_owned(bytes)
            .map_err(|_| "code-mode session tool-call budget is exhausted".to_string())?;
        let host = Arc::clone(&self.host_tool_byte_permits)
            .try_acquire_many_owned(bytes)
            .map_err(|_| "code-mode host tool-call budget is exhausted".to_string())?;
        Ok(ToolByteReservation {
            _session: session,
            _host: host,
        })
    }

    pub(super) fn buffered_tool_call(
        &self,
        message: proto::ToolCall,
        reservation: ToolByteReservation,
    ) -> BufferedToolCall {
        BufferedToolCall {
            message,
            _reservation: reservation,
        }
    }

    pub(super) async fn send_event(
        &self,
        event: proto::session_event::Event,
        cancellation: &CancellationToken,
    ) -> Result<(), String> {
        self.events.send(event, cancellation).await
    }

    pub(super) fn send_event_now(
        &self,
        event: proto::session_event::Event,
        cell_permit: Option<OwnedSemaphorePermit>,
    ) -> Result<(), String> {
        self.events.send_now(event, cell_permit)
    }

    pub(super) fn register_notification(
        &self,
        notification_id: Uuid,
        acknowledgement: oneshot::Sender<()>,
    ) -> Result<(), String> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if self.closed.is_cancelled() {
            return Err("code-mode session is closed".to_string());
        }
        if state.pending_notifications.contains_key(&notification_id)
            || !state.seen_notifications.remember(notification_id)
        {
            return Err("code-mode notification ID was reused".to_string());
        }
        state
            .pending_notifications
            .insert(notification_id, acknowledgement);
        Ok(())
    }

    pub(super) fn acknowledge_notification(
        &self,
        notification_id: Uuid,
    ) -> Result<(), Status> {
        let acknowledgement = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if self.closed.is_cancelled() {
                return Err(Status::cancelled("code-mode session is closed"));
            }
            match state.pending_notifications.remove(&notification_id) {
                Some(acknowledgement) => acknowledgement,
                None if state.seen_notifications.contains(&notification_id) => {
                    return Err(Status::already_exists(format!(
                        "code-mode notification {notification_id} was already retired"
                    )));
                }
                None => {
                    return Err(Status::not_found(format!(
                        "unknown code-mode notification {notification_id}"
                    )));
                }
            }
        };
        let _ = acknowledgement.send(());
        Ok(())
    }

    pub(super) fn cancel_notification(&self, notification_id: Uuid) {
        let pending = self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pending_notifications
            .remove(&notification_id);
        if pending.is_some() {
            let _ = self.send_event_now(
                proto::session_event::Event::NotificationCancelled(
                    proto::NotificationCancelled {
                        notification_id: notification_id.to_string(),
                    },
                ),
                /*cell_permit*/ None,
            );
        }
    }

    pub(super) fn discard_notification(&self, notification_id: Uuid) {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pending_notifications
            .remove(&notification_id);
    }
}

impl<T> BoundedIds<T>
where
    T: Clone + Eq + Hash,
{
    pub(super) fn remember(&mut self, id: T) -> bool {
        if !self.ids.insert(id.clone()) {
            return false;
        }
        self.order.push_back(id);
        while self.order.len() > MAX_RECENT_REQUEST_IDS {
            if let Some(expired) = self.order.pop_front() {
                self.ids.remove(&expired);
            }
        }
        true
    }

    pub(super) fn contains<Q>(&self, id: &Q) -> bool
    where
        T: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.ids.contains(id)
    }

    pub(super) fn remove<Q>(&mut self, id: &Q) -> bool
    where
        T: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        if !self.ids.remove(id) {
            return false;
        }
        self.order
            .retain(|queued| <T as Borrow<Q>>::borrow(queued) != id);
        true
    }
}
