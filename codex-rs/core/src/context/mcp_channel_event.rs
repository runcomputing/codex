use std::collections::BTreeMap;

use codex_mcp::McpChannelNotification;
use codex_protocol::user_input::UserInput;
use codex_utils_output_truncation::TruncationPolicy;
use codex_utils_output_truncation::truncate_text;

use super::ContextualUserFragment;

const MAX_CHANNEL_CONTENT_TOKENS: usize = 8_000;
const MAX_CHANNEL_META_ATTRIBUTES: usize = 32;
const MAX_CHANNEL_META_VALUE_TOKENS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct McpChannelEvent {
    source: String,
    content: String,
    meta: BTreeMap<String, String>,
}

impl McpChannelEvent {
    pub(crate) fn into_user_input(self) -> UserInput {
        UserInput::Text {
            text: self.render(),
            text_elements: Vec::new(),
        }
    }
}

impl From<McpChannelNotification> for McpChannelEvent {
    fn from(value: McpChannelNotification) -> Self {
        Self {
            source: value.source,
            content: value.content,
            meta: value
                .meta
                .into_iter()
                .filter(|(key, _)| key != "source")
                .collect(),
        }
    }
}

impl ContextualUserFragment for McpChannelEvent {
    fn role(&self) -> &'static str {
        "user"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        ("<channel", "</channel>")
    }

    fn body(&self) -> String {
        let mut attributes = format!(" source=\"{}\"", xml_escape(&self.source));
        for (key, value) in self.meta.iter().take(MAX_CHANNEL_META_ATTRIBUTES) {
            let value = truncate_text(
                value,
                TruncationPolicy::Tokens(MAX_CHANNEL_META_VALUE_TOKENS),
            );
            attributes.push_str(&format!(" {key}=\"{}\"", xml_escape(&value)));
        }

        // Talk/Claude-style channel content is often XML-escaped before it
        // reaches us. Decode it for readability, while preserving our wrapper
        // boundary if the message body mentions a closing channel tag.
        let content = decode_xml_entities(&self.content);
        let content = truncate_text(
            &content,
            TruncationPolicy::Tokens(MAX_CHANNEL_CONTENT_TOKENS),
        );
        format!("{attributes}>{}", escape_channel_end_markers(&content))
    }
}

fn xml_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn decode_xml_entities(value: &str) -> String {
    let mut decoded = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(amp_index) = rest.find('&') {
        decoded.push_str(&rest[..amp_index]);
        rest = &rest[amp_index..];
        let Some(semicolon_index) = rest.find(';') else {
            decoded.push_str(rest);
            return decoded;
        };

        let entity = &rest[1..semicolon_index];
        if let Some(ch) = decode_xml_entity(entity) {
            decoded.push(ch);
        } else {
            decoded.push_str(&rest[..=semicolon_index]);
        }
        rest = &rest[semicolon_index + 1..];
    }
    decoded.push_str(rest);
    decoded
}

fn decode_xml_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "apos" => Some('\''),
        "gt" => Some('>'),
        "lt" => Some('<'),
        "quot" => Some('"'),
        _ => decode_numeric_xml_entity(entity),
    }
}

fn decode_numeric_xml_entity(entity: &str) -> Option<char> {
    let codepoint = if let Some(hex) = entity
        .strip_prefix("#x")
        .or_else(|| entity.strip_prefix("#X"))
    {
        u32::from_str_radix(hex, 16).ok()?
    } else {
        entity.strip_prefix('#')?.parse().ok()?
    };
    char::from_u32(codepoint)
}

fn escape_channel_end_markers(value: &str) -> String {
    const END_MARKER: &str = "</channel>";
    let mut escaped = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(index) = find_ascii_case_insensitive(rest, END_MARKER) {
        escaped.push_str(&rest[..index]);
        escaped.push_str("&lt;/channel&gt;");
        rest = &rest[index + END_MARKER.len()..];
    }
    escaped.push_str(rest);
    escaped
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .as_bytes()
        .windows(needle.len())
        .position(|candidate| candidate.eq_ignore_ascii_case(needle.as_bytes()))
}
#[cfg(test)]
#[path = "mcp_channel_event_tests.rs"]
mod tests;
