use async_channel::Receiver;
use async_channel::Sender;
use codex_mcp::McpChannelNotification;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::Submission;
use codex_protocol::turn_input::TurnInputMode;
use codex_protocol::turn_input::TurnInputRequest;
use tokio::sync::oneshot;
use tracing::debug;
use uuid::Uuid;

use crate::context::McpChannelEvent;

/// An over-eager or buggy server must not be able to grow this queue without limit.
/// Senders use `try_send` and drop with a warning once it is full.
pub(super) const NOTIFICATION_CHANNEL_CAPACITY: usize = 64;

pub(super) fn spawn_mcp_channel_notification_loop(
    rx: Receiver<McpChannelNotification>,
    tx_sub: Sender<Submission>,
) {
    tokio::spawn(async move {
        while let Ok(notification) = rx.recv().await {
            let sub = channel_notification_submission(notification);
            if tx_sub.send(sub).await.is_err() {
                debug!("stopping MCP channel notification loop because submission queue closed");
                break;
            }
        }
    });
}

fn channel_notification_submission(notification: McpChannelNotification) -> Submission {
    let input = McpChannelEvent::from(notification).into_user_input();
    let (reply, _reply_rx) = oneshot::channel();
    Submission {
        id: Uuid::now_v7().to_string(),
        op: Op::TurnInput {
            request: Box::new(TurnInputRequest::user_input(vec![input])),
            mode: TurnInputMode::StartOrSteer,
            reply,
        },
        trace: None,
        parent_turn_id: None,
        root_turn_id: None,
    }
}
#[cfg(test)]
#[path = "mcp_channels_tests.rs"]
mod tests;
