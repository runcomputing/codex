use std::collections::BTreeMap;

use pretty_assertions::assert_eq;
use rmcp::model::CustomNotification;
use serde_json::json;

use super::*;

#[test]
fn parses_channel_notification_params() {
    let notification = CustomNotification::new(
        MCP_CHANNEL_NOTIFICATION_METHOD,
        Some(json!({
            "content": "hello",
            "meta": {
                "ack_required": "true",
                "arrived_on_channel": "worker-room",
                "reply_to_channel": "sender-room",
                "source": "server-supplied-source-is-dropped",
                "sent_at": "1710000000000",
                "bad-key": "dropped",
                "not_string": 5,
            },
        })),
    );

    assert_eq!(
        McpChannelNotification::from_custom_notification("talk", &notification)
            .expect("method should match")
            .expect("params should parse"),
        McpChannelNotification {
            source: "talk".to_string(),
            content: "hello".to_string(),
            meta: BTreeMap::from([
                ("ack_required".to_string(), "true".to_string()),
                ("arrived_on_channel".to_string(), "worker-room".to_string()),
                ("reply_to_channel".to_string(), "sender-room".to_string()),
                ("sent_at".to_string(), "1710000000000".to_string()),
            ]),
        }
    );
}

#[test]
fn ignores_other_custom_notifications() {
    let notification = CustomNotification::new(
        "notifications/talk/message",
        Some(json!({ "content": "hello" })),
    );

    assert_eq!(
        McpChannelNotification::from_custom_notification("talk", &notification),
        None
    );
}
