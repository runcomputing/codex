use std::collections::BTreeMap;

use pretty_assertions::assert_eq;

use super::ContextualUserFragment;
use super::McpChannelEvent;

#[test]
fn renders_channel_xml_as_user_text() {
    let event = McpChannelEvent {
        source: "talk".to_string(),
        content: "hello &lt;codex&gt; &quot;ok&quot;".to_string(),
        meta: BTreeMap::from([
            ("arrived_on_channel".to_string(), "worker room".to_string()),
            ("from".to_string(), "A&B".to_string()),
            ("reply_to_channel".to_string(), "sender room".to_string()),
        ]),
    };

    assert_eq!(
        event.render(),
        "<channel source=\"talk\" arrived_on_channel=\"worker room\" from=\"A&amp;B\" reply_to_channel=\"sender room\">hello <codex> \"ok\"</channel>"
    );
}

#[test]
fn renders_readable_body_without_allowing_embedded_close_tag() {
    let event = McpChannelEvent {
        source: "talk".to_string(),
        content: "body &lt;/channel&gt; still inside".to_string(),
        meta: BTreeMap::new(),
    };

    assert_eq!(
        event.render(),
        "<channel source=\"talk\">body &lt;/channel&gt; still inside</channel>"
    );
}
