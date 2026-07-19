use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::io::Error as IoError;
use std::io::ErrorKind;
use std::io::Result as IoResult;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;

use codex_app_server_protocol::RequestId;
use tokio::sync::oneshot;

use crate::RequestResult;

type ResponseSender = oneshot::Sender<IoResult<RequestResult>>;

struct PendingRequest {
    token: Arc<()>,
    response_tx: ResponseSender,
}

#[derive(Clone, Default)]
pub(super) struct PendingRequests {
    requests: Arc<Mutex<HashMap<RequestId, PendingRequest>>>,
}

pub(super) struct PendingRequestGuard {
    pending_requests: PendingRequests,
    request_id: RequestId,
    token: Arc<()>,
}

impl PendingRequests {
    pub(super) fn insert(
        &self,
        request_id: RequestId,
        response_tx: ResponseSender,
    ) -> IoResult<PendingRequestGuard> {
        let token = Arc::new(());
        match self.lock().entry(request_id) {
            Entry::Vacant(entry) => {
                let request_id = entry.key().clone();
                entry.insert(PendingRequest {
                    token: token.clone(),
                    response_tx,
                });
                Ok(PendingRequestGuard {
                    pending_requests: self.clone(),
                    request_id,
                    token,
                })
            }
            Entry::Occupied(entry) => Err(IoError::new(
                ErrorKind::InvalidInput,
                format!("duplicate remote app-server request id `{}`", entry.key()),
            )),
        }
    }

    pub(super) fn contains(&self, request_id: &RequestId) -> bool {
        self.lock().contains_key(request_id)
    }

    pub(super) fn remove(&self, request_id: &RequestId) -> Option<ResponseSender> {
        self.lock()
            .remove(request_id)
            .map(|request| request.response_tx)
    }

    pub(super) fn drain(&self) -> Vec<ResponseSender> {
        std::mem::take(&mut *self.lock())
            .into_values()
            .map(|request| request.response_tx)
            .collect()
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<RequestId, PendingRequest>> {
        self.requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Drop for PendingRequestGuard {
    fn drop(&mut self) {
        let mut requests = self.pending_requests.lock();
        if requests
            .get(&self.request_id)
            .is_some_and(|request| Arc::ptr_eq(&request.token, &self.token))
        {
            requests.remove(&self.request_id);
        }
    }
}
