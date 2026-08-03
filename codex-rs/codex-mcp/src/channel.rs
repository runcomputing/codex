use std::collections::BTreeMap;

use rmcp::model::CustomNotification;
use serde_json::Value;
use thiserror::Error;

pub const MCP_CHANNEL_CAPABILITY: &str = "codex/channel";
pub const MCP_CHANNEL_NOTIFICATION_METHOD: &str = "notifications/codex/channel";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpChannelNotification {
    pub source: String,
    pub content: String,
    pub meta: BTreeMap<String, String>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum McpChannelNotificationParseError {
    #[error("missing params")]
    MissingParams,
    #[error("params must be a JSON object")]
    InvalidParams,
    #[error("params.content must be a string")]
    InvalidContent,
}

impl McpChannelNotification {
    pub(crate) fn from_custom_notification(
        source: &str,
        notification: &CustomNotification,
    ) -> Option<Result<Self, McpChannelNotificationParseError>> {
        if notification.method != MCP_CHANNEL_NOTIFICATION_METHOD {
            return None;
        }

        Some(parse_channel_notification(
            source,
            notification.params.as_ref(),
        ))
    }
}

fn parse_channel_notification(
    source: &str,
    params: Option<&Value>,
) -> Result<McpChannelNotification, McpChannelNotificationParseError> {
    let Some(params) = params else {
        return Err(McpChannelNotificationParseError::MissingParams);
    };
    let Value::Object(params) = params else {
        return Err(McpChannelNotificationParseError::InvalidParams);
    };
    let Some(content) = params.get("content").and_then(Value::as_str) else {
        return Err(McpChannelNotificationParseError::InvalidContent);
    };

    let meta = params
        .get("meta")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|meta| meta.iter())
        .filter_map(|(key, value)| {
            if is_valid_channel_meta_key(key) {
                value.as_str().map(|value| (key.clone(), value.to_string()))
            } else {
                None
            }
        })
        .collect();

    Ok(McpChannelNotification {
        source: source.to_string(),
        content: content.to_string(),
        meta,
    })
}

fn is_valid_channel_meta_key(key: &str) -> bool {
    key != "source"
        && !key.is_empty()
        && key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}
#[cfg(test)]
#[path = "channel_tests.rs"]
mod tests;
