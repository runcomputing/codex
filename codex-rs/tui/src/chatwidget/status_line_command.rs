//! External status-line command lifecycle for `ChatWidget`.
//!
//! A configured command runs outside Codex's sandbox with an argv vector rather
//! than a shell string. Its output is treated as untrusted one-line text and is
//! bounded before it reaches the TUI.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::process::Child;
use tokio::process::Command;

use super::ChatWidget;
use crate::app_event::AppEvent;

const STATUS_LINE_COMMAND_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const STATUS_LINE_COMMAND_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_STATUS_LINE_COMMAND_OUTPUT_BYTES: usize = 1_024;
const MAX_STATUS_LINE_COMMAND_OUTPUT_CHARS: usize = 240;

/// Inputs that identify one external status-line command invocation.
///
/// The value doubles as the cache key: output is discarded whenever the command,
/// active thread, or current directory changes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct StatusLineCommandContext {
    command: Vec<String>,
    cwd: PathBuf,
    thread_id: Option<String>,
}

impl StatusLineCommandContext {
    pub(super) fn new(
        command: Vec<String>,
        cwd: PathBuf,
        thread_id: Option<String>,
    ) -> Option<Self> {
        let program = command.first()?;
        if program.trim().is_empty() {
            return None;
        }

        Some(Self {
            command,
            cwd,
            thread_id,
        })
    }
}

impl ChatWidget {
    /// Synchronizes the external-command cache with the current status-line selection.
    pub(super) fn sync_status_line_command_state(&mut self, enabled: bool) {
        let context = if enabled {
            self.config
                .tui_status_line_command
                .clone()
                .and_then(|command| {
                    StatusLineCommandContext::new(
                        command,
                        self.status_line_cwd().to_path_buf(),
                        self.thread_id.as_ref().map(ToString::to_string),
                    )
                })
        } else {
            None
        };

        if self.status_line_command_context.as_ref() != context.as_ref() {
            self.status_line_command_context = context;
            self.status_line_command_output = None;
            self.status_line_command_pending_request_id = None;
            self.status_line_command_last_requested_at = None;
        }

        self.request_status_line_command_if_due(Instant::now());
    }

    /// Starts a refresh from a scheduled TUI frame when the command is due.
    pub(super) fn refresh_status_line_command_if_due(&mut self) {
        let now = Instant::now();
        if self.status_line_command_refresh_is_due(now) {
            self.refresh_status_line();
            return;
        }

        if self.status_line_command_context.is_some()
            && self.status_line_command_pending_request_id.is_none()
            && let Some(last_requested_at) = self.status_line_command_last_requested_at
        {
            let elapsed = now.saturating_duration_since(last_requested_at);
            self.frame_requester
                .schedule_frame_in(STATUS_LINE_COMMAND_REFRESH_INTERVAL.saturating_sub(elapsed));
        }
    }

    /// Stores the latest external-command output when it belongs to the active request.
    pub(crate) fn set_status_line_command_output(
        &mut self,
        request_id: u64,
        output: Option<String>,
    ) -> bool {
        if self.status_line_command_pending_request_id != Some(request_id) {
            return false;
        }

        self.status_line_command_pending_request_id = None;
        self.status_line_command_output = output;
        true
    }

    fn request_status_line_command_if_due(&mut self, now: Instant) {
        if !self.status_line_command_refresh_is_due(now) {
            return;
        }

        let Some(context) = self.status_line_command_context.clone() else {
            return;
        };
        let request_id = self.next_status_line_command_request_id;
        self.next_status_line_command_request_id = self
            .next_status_line_command_request_id
            .wrapping_add(/*rhs*/ 1);
        self.status_line_command_pending_request_id = Some(request_id);
        self.status_line_command_last_requested_at = Some(now);

        let tx = self.app_event_tx.clone();
        tokio::spawn(async move {
            let output = run_status_line_command(context).await;
            tx.send(AppEvent::StatusLineCommandUpdated { request_id, output });
        });
        self.frame_requester
            .schedule_frame_in(STATUS_LINE_COMMAND_REFRESH_INTERVAL);
    }

    fn status_line_command_refresh_is_due(&self, now: Instant) -> bool {
        self.status_line_command_context.is_some()
            && self.status_line_command_pending_request_id.is_none()
            && self
                .status_line_command_last_requested_at
                .is_none_or(|last_requested_at| {
                    now.saturating_duration_since(last_requested_at)
                        >= STATUS_LINE_COMMAND_REFRESH_INTERVAL
                })
    }
}

pub(super) async fn run_status_line_command(context: StatusLineCommandContext) -> Option<String> {
    let mut child = match status_line_command_process(&context)?.spawn() {
        Ok(child) => child,
        Err(error) => {
            tracing::trace!(%error, "failed to start status-line command");
            return None;
        }
    };
    let Some(stdout) = child.stdout.take() else {
        tracing::trace!("status-line command did not expose stdout");
        return None;
    };

    let result = tokio::time::timeout(STATUS_LINE_COMMAND_TIMEOUT, async {
        tokio::try_join!(child.wait(), read_capped_output(stdout))
    })
    .await;
    let (status, stdout) = match result {
        Ok(Ok((status, stdout))) => (status, stdout),
        Ok(Err(error)) => {
            stop_status_line_command(&mut child).await;
            tracing::trace!(%error, "failed to read or wait for status-line command");
            return None;
        }
        Err(_) => {
            stop_status_line_command(&mut child).await;
            tracing::trace!("status-line command timed out");
            return None;
        }
    };
    if !status.success() {
        tracing::trace!(?status, "status-line command exited unsuccessfully");
        return None;
    }

    status_line_command_output(&stdout)
}

fn status_line_command_process(context: &StatusLineCommandContext) -> Option<Command> {
    let (program, args) = context.command.split_first()?;
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(&context.cwd)
        .env("CODEX_STATUS_LINE_CWD", &context.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    if let Some(thread_id) = &context.thread_id {
        command.env("CODEX_THREAD_ID", thread_id);
    } else {
        command.env_remove("CODEX_THREAD_ID");
    }
    Some(command)
}

async fn stop_status_line_command(child: &mut Child) {
    if let Err(error) = child.kill().await {
        tracing::trace!(%error, "failed to stop status-line command");
    }
}

async fn read_capped_output<R>(mut reader: R) -> std::io::Result<Vec<u8>>
where
    R: AsyncRead + Unpin,
{
    let mut output = Vec::with_capacity(MAX_STATUS_LINE_COMMAND_OUTPUT_BYTES);
    let mut buffer = [0; 512];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            return Ok(output);
        }

        let retained = MAX_STATUS_LINE_COMMAND_OUTPUT_BYTES
            .saturating_sub(output.len())
            .min(read);
        output.extend_from_slice(&buffer[..retained]);
    }
}

fn status_line_command_output(stdout: &[u8]) -> Option<String> {
    let first_line = String::from_utf8_lossy(stdout).lines().next()?.to_string();
    let mut output = String::new();
    let mut characters = first_line.chars();
    while let Some(character) = characters.next() {
        if character == '\x1b' {
            match characters.next() {
                Some('[') => {
                    for character in characters.by_ref() {
                        if ('@'..='~').contains(&character) {
                            break;
                        }
                    }
                }
                Some(']') | Some('P') | Some('^') | Some('_') => {
                    let mut escaped = false;
                    for character in characters.by_ref() {
                        if character == '\x07' || (escaped && character == '\\') {
                            break;
                        }
                        escaped = character == '\x1b';
                    }
                }
                Some(_) | None => {}
            }
            continue;
        }

        if !character.is_control() {
            output.push(character);
        }
    }

    let output = output.trim();
    (!output.is_empty()).then(|| {
        output
            .chars()
            .take(MAX_STATUS_LINE_COMMAND_OUTPUT_CHARS)
            .collect()
    })
}

#[cfg(test)]
#[path = "status_line_command_tests.rs"]
mod tests;
