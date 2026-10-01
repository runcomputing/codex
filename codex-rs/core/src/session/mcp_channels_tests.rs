use std::collections::BTreeMap;

use codex_mcp::McpChannelNotification;
use codex_protocol::protocol::Op;
use codex_protocol::turn_input::TurnInput as SubmittedTurnInput;
use codex_protocol::turn_input::TurnInputMode;
use codex_protocol::user_input::UserInput;
use pretty_assertions::assert_eq;

use super::channel_notification_submission;

#[test]
fn channel_notification_becomes_user_input_submission() {
    let sub = channel_notification_submission(McpChannelNotification {
        source: "talk".to_string(),
        content: "hello".to_string(),
        meta: BTreeMap::from([
            ("arrived_on_channel".to_string(), "desk".to_string()),
            ("reply_to_channel".to_string(), "user".to_string()),
            ("source".to_string(), "not-the-server-name".to_string()),
        ]),
    });

    let Op::TurnInput { request, mode, .. } = sub.op else {
        panic!("expected turn input op");
    };
    assert_eq!(mode, TurnInputMode::StartOrSteer);
    let SubmittedTurnInput::UserInput { content, client_id } = request.input else {
        panic!("expected user input");
    };
    assert_eq!(client_id, None);

    assert_eq!(
        content,
        vec![UserInput::Text {
            text: "<channel source=\"talk\" arrived_on_channel=\"desk\" reply_to_channel=\"user\">hello</channel>".to_string(),
            text_elements: Vec::new(),
        }]
    );
}
