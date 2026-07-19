use std::io::Error as IoError;
use std::io::ErrorKind;
use std::io::Result as IoResult;

use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ServerRequest;
use tokio::sync::mpsc;
use tracing::warn;

use crate::AppServerEvent;
use crate::server_notification_requires_delivery;

pub(super) fn channel(capacity: usize) -> (RemoteEventSender, mpsc::Receiver<AppServerEvent>) {
    let (tx, rx) = mpsc::channel(capacity);
    (
        RemoteEventSender {
            tx,
            dropped: DroppedEventCounts::default(),
        },
        rx,
    )
}

pub(super) struct RemoteEventSender {
    tx: mpsc::Sender<AppServerEvent>,
    dropped: DroppedEventCounts,
}

#[derive(Default)]
struct DroppedEventCounts {
    total: usize,
    reportable: usize,
}

impl RemoteEventSender {
    pub(super) async fn send(&mut self, event: AppServerEvent) -> IoResult<Option<ServerRequest>> {
        if event_requires_delivery(&event) {
            if self.dropped.reportable > 0 {
                self.tx
                    .send(AppServerEvent::Lagged {
                        skipped: self.dropped.total,
                    })
                    .await
                    .map_err(|_| consumer_closed_error())?;
            }
            self.dropped = DroppedEventCounts::default();
            self.tx
                .send(event)
                .await
                .map_err(|_| consumer_closed_error())?;
            return Ok(None);
        }

        match self.tx.try_send(event) {
            Ok(()) => Ok(None),
            Err(mpsc::error::TrySendError::Full(event)) => {
                self.dropped.total = self.dropped.total.saturating_add(1);
                if matches!(
                    &event,
                    AppServerEvent::ServerNotification(
                        ServerNotification::CommandExecutionOutputDelta(_)
                    )
                ) {
                    tracing::debug!(
                        "dropping remote command output because consumer queue is full"
                    );
                } else {
                    self.dropped.reportable = self.dropped.reportable.saturating_add(1);
                    warn!("dropping remote app-server event because consumer queue is full");
                }
                match event {
                    AppServerEvent::ServerRequest(request) => Ok(Some(request)),
                    AppServerEvent::Lagged { .. }
                    | AppServerEvent::ServerNotification(_)
                    | AppServerEvent::Disconnected { .. } => Ok(None),
                }
            }
            Err(mpsc::error::TrySendError::Closed(_)) => Err(consumer_closed_error()),
        }
    }
}

fn event_requires_delivery(event: &AppServerEvent) -> bool {
    match event {
        AppServerEvent::ServerNotification(notification) => {
            server_notification_requires_delivery(notification)
        }
        AppServerEvent::Disconnected { .. } => true,
        AppServerEvent::Lagged { .. } | AppServerEvent::ServerRequest(_) => false,
    }
}

fn consumer_closed_error() -> IoError {
    IoError::new(
        ErrorKind::BrokenPipe,
        "remote app-server event consumer channel is closed",
    )
}

#[cfg(test)]
#[path = "event_queue_tests.rs"]
mod tests;
