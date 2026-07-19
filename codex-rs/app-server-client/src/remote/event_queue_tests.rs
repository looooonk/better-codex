use super::*;
use codex_app_server_protocol::AccountUpdatedNotification;
use codex_app_server_protocol::AgentMessageDeltaNotification;
use codex_app_server_protocol::CommandExecutionOutputDeltaNotification;
use codex_app_server_protocol::CurrentTimeReadParams;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ServerRequest;

fn account_updated() -> AppServerEvent {
    AppServerEvent::ServerNotification(ServerNotification::AccountUpdated(
        AccountUpdatedNotification {
            auth_mode: None,
            plan_type: None,
        },
    ))
}

fn agent_delta(text: &str) -> AppServerEvent {
    AppServerEvent::ServerNotification(ServerNotification::AgentMessageDelta(
        AgentMessageDeltaNotification {
            thread_id: "thread".to_string(),
            turn_id: "turn".to_string(),
            item_id: "item".to_string(),
            delta: text.to_string(),
        },
    ))
}

fn command_delta(text: &str) -> AppServerEvent {
    AppServerEvent::ServerNotification(ServerNotification::CommandExecutionOutputDelta(
        CommandExecutionOutputDeltaNotification {
            thread_id: "thread".to_string(),
            turn_id: "turn".to_string(),
            item_id: "item".to_string(),
            delta: text.to_string(),
        },
    ))
}

#[tokio::test]
async fn full_queue_drops_best_effort_event_and_reports_lag_before_lossless_event() {
    let (mut tx, mut rx) = channel(1);
    assert!(tx.send(account_updated()).await.unwrap().is_none());
    assert!(tx.send(account_updated()).await.unwrap().is_none());
    assert!(matches!(
        rx.recv().await,
        Some(AppServerEvent::ServerNotification(
            ServerNotification::AccountUpdated(_)
        ))
    ));

    let send = tokio::spawn(async move { tx.send(agent_delta("hello")).await });
    assert!(matches!(
        rx.recv().await,
        Some(AppServerEvent::Lagged { skipped: 1 })
    ));
    assert!(matches!(
        rx.recv().await,
        Some(AppServerEvent::ServerNotification(
            ServerNotification::AgentMessageDelta(notification)
        )) if notification.delta == "hello"
    ));
    assert!(send.await.unwrap().unwrap().is_none());
}

#[tokio::test]
async fn dropped_command_output_does_not_add_lag_notice() {
    let (mut tx, mut rx) = channel(1);
    assert!(tx.send(command_delta("first")).await.unwrap().is_none());
    assert!(tx.send(command_delta("second")).await.unwrap().is_none());
    assert!(matches!(
        rx.recv().await,
        Some(AppServerEvent::ServerNotification(
            ServerNotification::CommandExecutionOutputDelta(notification)
        )) if notification.delta == "first"
    ));

    assert!(tx.send(agent_delta("hello")).await.unwrap().is_none());
    assert!(matches!(
        rx.recv().await,
        Some(AppServerEvent::ServerNotification(
            ServerNotification::AgentMessageDelta(notification)
        )) if notification.delta == "hello"
    ));
}

#[tokio::test]
async fn full_queue_returns_server_request_for_rejection() {
    let (mut tx, _rx) = channel(1);
    assert!(tx.send(account_updated()).await.unwrap().is_none());

    let dropped = tx
        .send(AppServerEvent::ServerRequest(
            ServerRequest::CurrentTimeRead {
                request_id: RequestId::Integer(7),
                params: CurrentTimeReadParams {
                    thread_id: "thread".to_string(),
                },
            },
        ))
        .await
        .unwrap();
    assert!(matches!(
        dropped,
        Some(ServerRequest::CurrentTimeRead {
            request_id: RequestId::Integer(7),
            ..
        })
    ));
}
