//! Streaming primitives used by the TUI transcript pipeline.
//!
//! `StreamState` owns newline-gated markdown collection and a FIFO queue of committed render lines.
//! Higher-level modules build on top of this state:
//! - `controller` adapts queued lines into `HistoryCell` emission rules for message and plan streams.
//! - `chunking` computes adaptive drain plans from queue pressure.
//! - `commit_tick` binds policy decisions to concrete controller drains.
//!
//! The key invariant is queue ordering. All drains pop from the front, and enqueue records an
//! arrival timestamp so policy code can reason about oldest queued age without peeking into text.

use std::collections::VecDeque;
use std::path::Path;
use std::time::Duration;
use std::time::Instant;

use crate::markdown_stream::MarkdownStreamCollector;
use crate::terminal_hyperlinks::HyperlinkLine;
pub(crate) mod chunking;
pub(crate) mod commit_tick;
pub(crate) mod controller;
mod render;
mod table_holdback;

struct QueuedLine {
    line: HyperlinkLine,
    enqueued_at: Instant,
}

/// Holds in-flight markdown stream state and queued committed lines.
pub(crate) struct StreamState {
    pub(crate) collector: MarkdownStreamCollector,
    queued_lines: VecDeque<QueuedLine>,
    pub(crate) has_seen_delta: bool,
}

impl StreamState {
    /// Create stream state whose markdown collector renders local file links relative to `cwd`.
    ///
    /// Controllers are expected to pass the session cwd here once and keep it stable for the
    /// lifetime of the active stream.
    pub(crate) fn new(width: Option<usize>, cwd: &Path) -> Self {
        Self {
            collector: MarkdownStreamCollector::new(width, cwd),
            queued_lines: VecDeque::new(),
            has_seen_delta: false,
        }
    }
    /// Resets collector and queue state for the next stream lifecycle.
    pub(crate) fn clear(&mut self) {
        self.collector.clear();
        self.queued_lines.clear();
        self.has_seen_delta = false;
    }
    /// Drains one semantic render unit from the front of the queue.
    ///
    /// Plain text contributes one line. A terminal image contributes its complete multi-row
    /// footprint so a commit tick cannot split the image during the handoff to scrollback.
    pub(crate) fn step(&mut self) -> Vec<HyperlinkLine> {
        self.drain_n(/*max_lines*/ 1)
    }
    /// Drains up to `max_lines` queued lines from the front of the queue.
    ///
    /// When that boundary falls inside a terminal image, the drain extends through the image's
    /// complete footprint. Callers that pass very large values still get bounded behavior because
    /// this method clamps to the currently available queue length.
    pub(crate) fn drain_n(&mut self, max_lines: usize) -> Vec<HyperlinkLine> {
        let mut end = max_lines.min(self.queued_lines.len());
        let mut index = 0;
        while index < end {
            if let Some(group_height) = self.terminal_image_group_height(index) {
                end = end.max(index + group_height);
                index += group_height;
            } else {
                index += 1;
            }
        }
        self.queued_lines
            .drain(..end)
            .map(|queued| queued.line)
            .collect()
    }

    /// Return the complete image footprint beginning at `start` when it is already queued.
    ///
    /// A Kitty image is one semantic render split across logical transcript rows. Letting a commit
    /// tick stop inside that group makes the active-to-scrollback handoff redraw it one row at a
    /// time, which corrupts placement in terminals that support the protocol.
    fn terminal_image_group_height(&self, start: usize) -> Option<usize> {
        let crate::terminal_render::TerminalLineRender::Image { columns, image } = self
            .queued_lines
            .get(start)?
            .line
            .terminal_render
            .as_ref()?
        else {
            return None;
        };
        let height = usize::from(image.size().height);
        if height < 2 || start + height > self.queued_lines.len() {
            return None;
        }
        for row in 1..height {
            let Some(crate::terminal_render::TerminalLineRender::ImageRow {
                columns: row_columns,
                image: row_image,
                row: image_row,
            }) = self
                .queued_lines
                .get(start + row)?
                .line
                .terminal_render
                .as_ref()
            else {
                return None;
            };
            if row_columns != columns || row_image != image || usize::from(*image_row) != row {
                return None;
            }
        }
        Some(height)
    }
    /// Clears queued lines while keeping collector/turn lifecycle state intact.
    pub(crate) fn clear_queue(&mut self) {
        self.queued_lines.clear();
    }
    /// Returns whether no lines are queued for commit.
    pub(crate) fn is_idle(&self) -> bool {
        self.queued_lines.is_empty()
    }
    /// Returns the current queue depth.
    pub(crate) fn queued_len(&self) -> usize {
        self.queued_lines.len()
    }
    /// Returns the age of the oldest queued line.
    pub(crate) fn oldest_queued_age(&self, now: Instant) -> Option<Duration> {
        self.queued_lines
            .front()
            .map(|queued| now.saturating_duration_since(queued.enqueued_at))
    }
    /// Appends committed lines to the queue with a shared enqueue timestamp.
    pub(crate) fn enqueue(&mut self, lines: Vec<HyperlinkLine>) {
        let now = Instant::now();
        self.queued_lines
            .extend(lines.into_iter().map(|line| QueuedLine {
                line,
                enqueued_at: now,
            }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::DynamicImage;
    use pretty_assertions::assert_eq;
    use ratatui::layout::Size;
    use ratatui::text::Line;
    use std::path::PathBuf;

    fn test_cwd() -> PathBuf {
        // These tests only need a stable absolute cwd; using temp_dir() avoids baking Unix- or
        // Windows-specific root semantics into the fixtures.
        std::env::temp_dir()
    }

    #[test]
    fn drain_n_clamps_to_available_lines() {
        let mut state = StreamState::new(/*width*/ None, &test_cwd());
        state.enqueue(vec![HyperlinkLine::new(Line::from("one"))]);

        let drained = state.drain_n(/*max_lines*/ 8);
        assert_eq!(drained, vec![HyperlinkLine::new(Line::from("one"))]);
        assert!(state.is_idle());
    }

    #[test]
    fn single_step_keeps_terminal_image_rows_together() {
        let image = crate::terminal_render::TerminalImage::new(
            DynamicImage::new_rgba8(/*width*/ 40, /*height*/ 60),
            Size::new(/*width*/ 4, /*height*/ 3),
        )
        .expect("terminal image");
        let mut lines = Vec::new();
        let mut first = HyperlinkLine::new(Line::from("    "));
        first.terminal_render = Some(crate::terminal_render::TerminalLineRender::Image {
            columns: 0..4,
            image: image.clone(),
        });
        lines.push(first);
        for row in 1..3 {
            let mut continuation = HyperlinkLine::new(Line::from("    "));
            continuation.terminal_render =
                Some(crate::terminal_render::TerminalLineRender::ImageRow {
                    columns: 0..4,
                    image: image.clone(),
                    row,
                });
            lines.push(continuation);
        }
        lines.push(HyperlinkLine::new(Line::from("after")));
        let mut state = StreamState::new(/*width*/ None, &test_cwd());
        state.enqueue(lines);

        let drained = state.step();

        assert_eq!(drained.len(), 3);
        assert_eq!(state.queued_len(), 1);
    }
}
