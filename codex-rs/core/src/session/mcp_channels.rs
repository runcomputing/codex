use std::collections::BTreeMap;

use async_channel::Receiver;
use async_channel::Sender;
use codex_mcp::McpChannelNotification;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::Submission;
use codex_protocol::protocol::ThreadSettingsOverrides;
use tracing::debug;
use uuid::Uuid;

use crate::context::McpChannelEvent;

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
    Submission {
        id: Uuid::now_v7().to_string(),
        op: Op::UserInput {
            items: vec![input],
            final_output_json_schema: None,
            responsesapi_client_metadata: None,
            additional_context: BTreeMap::new(),
            thread_settings: ThreadSettingsOverrides::default(),
        },
        client_user_message_id: None,
        trace: None,
    }
}
#[cfg(test)]
#[path = "mcp_channels_tests.rs"]
mod tests;
