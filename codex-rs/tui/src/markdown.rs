//! Markdown-to-ratatui rendering entry points.
//!
//! This module provides the public API surface that the rest of the TUI uses
//! to turn markdown source into `Vec<Line<'static>>`.  Two variants exist:
//!
//! - [`append_markdown`] -- general-purpose, used for plan blocks and history
//!   cells that already hold pre-processed markdown (no fence unwrapping).
//! - [`append_markdown_agent`] -- for agent responses.  Runs
//!   [`unwrap_markdown_fences`] first so that `` ```md ``/`` ```markdown ``
//!   fences containing tables, plus whole-document Markdown envelopes containing nested fences,
//!   are stripped so `pulldown-cmark` sees their Markdown structure instead of fenced code.
//!
//! ## Why fence unwrapping exists
//!
//! LLM agents frequently wrap tables in `` ```markdown `` fences, treating
//! them as code.  Without unwrapping, `pulldown-cmark` parses those lines
//! as a fenced code block and renders them as monospace code rather than a
//! structured table.  The unwrapper is intentionally conservative: it
//! buffers the entire fence body before deciding and degrades gracefully on unclosed fences.
use ratatui::text::Line;
use std::borrow::Cow;
use std::ops::Range;
use std::path::Path;

use crate::inline_visualization::InlineVisualizationContext;
use crate::inline_visualization::rewrite_inline_visualizations;
use crate::table_detect;
use crate::terminal_hyperlinks::HyperlinkLine;

/// Render markdown source to styled ratatui lines and append them to `lines`.
///
/// Callers that already know the session working directory should pass it here so streamed and
/// non-streamed rendering show the same relative path text even if the process cwd differs.
pub(crate) fn append_markdown(
    markdown_source: &str,
    width: Option<usize>,
    cwd: Option<&Path>,
    lines: &mut Vec<Line<'static>>,
) {
    let rendered = crate::markdown_render::render_markdown_text_with_width_and_cwd(
        markdown_source,
        width,
        cwd,
    );
    crate::render::line_utils::push_owned_lines(&rendered.lines, lines);
}

/// Render an agent message to styled ratatui lines.
///
/// Before rendering, the source is passed through [`unwrap_markdown_fences`] so that tables
/// wrapped in `` ```md `` fences are rendered as native tables rather than code blocks.
/// Non-markdown fences (e.g. `rust`, `sh`) are left
/// intact.
#[cfg(test)]
pub(crate) fn append_markdown_agent(
    markdown_source: &str,
    width: Option<usize>,
    lines: &mut Vec<Line<'static>>,
) {
    let normalized = unwrap_markdown_fences(markdown_source);
    let rendered = crate::markdown_render::render_markdown_text_with_width_and_cwd(
        &normalized,
        width,
        /*cwd*/ None,
    );
    crate::render::line_utils::push_owned_lines(&rendered.lines, lines);
}

#[cfg(test)]
pub(crate) fn render_markdown_agent_with_links_cwd_and_visualizations(
    markdown_source: &str,
    width: Option<usize>,
    cwd: Option<&Path>,
    inline_visualization_context: Option<&InlineVisualizationContext>,
) -> Vec<HyperlinkLine> {
    render_markdown_agent_with_links_cwd_visualizations_and_options(
        markdown_source,
        width,
        cwd,
        inline_visualization_context,
        crate::markdown_render::MarkdownRenderOptions::default(),
    )
}

pub(crate) fn render_markdown_agent_with_links_cwd_visualizations_and_options(
    markdown_source: &str,
    width: Option<usize>,
    cwd: Option<&Path>,
    inline_visualization_context: Option<&InlineVisualizationContext>,
    render_options: crate::markdown_render::MarkdownRenderOptions,
) -> Vec<HyperlinkLine> {
    let rewritten = rewrite_inline_visualizations(markdown_source, inline_visualization_context);
    let normalized = unwrap_markdown_fences(&rewritten.markdown);
    let is_hidden_link_destination =
        |destination: &str| rewritten.trusted_file_links.contains_key(destination);
    let mut lines = crate::markdown_render::render_markdown_lines_with_options(
        &normalized,
        width,
        cwd,
        &is_hidden_link_destination,
        render_options,
    );
    for hyperlink in lines.iter_mut().flat_map(|line| &mut line.hyperlinks) {
        if let Some(link) = rewritten.trusted_file_links.get(&hyperlink.destination) {
            hyperlink.retarget_to_trusted_file(&link.destination);
        }
    }
    lines
}

/// Render an agent message and collect the block metadata needed for incremental rendering.
///
/// Block offsets are mapped back to `markdown_source` after Markdown table fences are unwrapped.
/// If a normalized boundary cannot be expressed as a raw-source suffix, it is discarded so the
/// transformed block remains mutable.
#[cfg(test)]
pub(crate) fn render_streaming_markdown_agent_with_links_and_cwd(
    markdown_source: &str,
    width: Option<usize>,
    cwd: Option<&Path>,
) -> crate::markdown_render::StreamingMarkdownRender {
    render_streaming_markdown_agent_with_links_cwd_and_options(
        markdown_source,
        width,
        cwd,
        crate::markdown_render::MarkdownRenderOptions::default(),
    )
}

pub(crate) fn render_streaming_markdown_agent_with_links_cwd_and_options(
    markdown_source: &str,
    width: Option<usize>,
    cwd: Option<&Path>,
    render_options: crate::markdown_render::MarkdownRenderOptions,
) -> crate::markdown_render::StreamingMarkdownRender {
    let normalized = unwrap_markdown_fences(markdown_source);
    let mut rendered = crate::markdown_render::render_streaming_markdown_lines_with_width_and_cwd(
        &normalized,
        width,
        cwd,
        render_options,
    );
    if normalized != markdown_source {
        // Fence unwrapping removes opening/closing lines. A normalized tail that is still a raw
        // suffix necessarily begins after those removed lines, so its boundary can safely be
        // mapped back to the raw source; otherwise leave the transformed block mutable.
        rendered.last_top_level_block_start = rendered
            .last_top_level_block_start
            .and_then(|boundary| markdown_source.strip_suffix(&normalized[boundary..]))
            .map(str::len);
    }
    rendered
}

/// Strip selected `` ```md ``/`` ```markdown `` wrappers, emitting their content as bare Markdown.
///
/// Table fences are unwrapped so `pulldown-cmark` parses the table natively. A fence that encloses
/// the whole response and contains a nested fenced block is also unwrapped; models commonly use
/// that shape to return a complete Markdown document containing code samples. Other Markdown
/// fences remain code blocks.
///
/// The fence unwrapping is intentionally conservative: it buffers the entire fence body before
/// deciding, and an unclosed fence at end-of-input is re-emitted with its opening line so partial
/// streams degrade to code display.
fn unwrap_markdown_fences<'a>(markdown_source: &'a str) -> Cow<'a, str> {
    // Zero-copy fast path: most messages contain no fences at all.
    if !markdown_source.contains("```") && !markdown_source.contains("~~~") {
        return Cow::Borrowed(markdown_source);
    }

    #[derive(Clone, Copy)]
    struct Fence {
        marker: u8,
        len: usize,
        is_blockquoted: bool,
    }

    // Strip a trailing newline and up to 3 leading spaces, returning the
    // trimmed slice.  Returns `None` when the line has 4+ leading spaces
    // (which makes it an indented code line per CommonMark).
    fn strip_line_indent(line: &str) -> Option<&str> {
        let without_newline = line.strip_suffix('\n').unwrap_or(line);
        let mut byte_idx = 0usize;
        let mut column = 0usize;
        for b in without_newline.as_bytes() {
            match b {
                b' ' => {
                    byte_idx += 1;
                    column += 1;
                }
                b'\t' => {
                    byte_idx += 1;
                    column += 4;
                }
                _ => break,
            }
            if column >= 4 {
                return None;
            }
        }
        Some(&without_newline[byte_idx..])
    }

    // Parse an opening fence line, returning the fence metadata and whether
    // the fence info string indicates markdown content.
    fn parse_open_fence(line: &str) -> Option<(Fence, bool)> {
        let trimmed = strip_line_indent(line)?;
        let is_blockquoted = trimmed.trim_start().starts_with('>');
        let fence_scan_text = table_detect::strip_blockquote_prefix(trimmed);
        let (marker, len) = table_detect::parse_fence_marker(fence_scan_text)?;
        let is_markdown = table_detect::is_markdown_fence_info(fence_scan_text, len);
        Some((
            Fence {
                marker: marker as u8,
                len,
                is_blockquoted,
            },
            is_markdown,
        ))
    }

    fn is_close_fence(line: &str, fence: Fence) -> bool {
        let Some(trimmed) = strip_line_indent(line) else {
            return false;
        };
        let fence_scan_text = if fence.is_blockquoted {
            if !trimmed.trim_start().starts_with('>') {
                return false;
            }
            table_detect::strip_blockquote_prefix(trimmed)
        } else {
            trimmed
        };
        if let Some((marker, len)) = table_detect::parse_fence_marker(fence_scan_text) {
            marker as u8 == fence.marker
                && len >= fence.len
                && fence_scan_text[len..].trim().is_empty()
        } else {
            false
        }
    }

    fn whole_document_markdown_envelope_body(markdown_source: &str) -> Option<&str> {
        let trimmed = markdown_source.trim();
        let opening_end = trimmed.find('\n')?;
        let opening_line = &trimmed[..opening_end];
        let (fence, is_markdown) = parse_open_fence(opening_line)?;
        if !is_markdown || fence.is_blockquoted {
            return None;
        }

        let closing_start = trimmed.rfind('\n')?.checked_add(1)?;
        if closing_start <= opening_end || !is_close_fence(&trimmed[closing_start..], fence) {
            return None;
        }
        let body = &trimmed[opening_end + 1..closing_start];
        let contains_shorter_nested_fence = body.lines().any(|line| {
            parse_open_fence(line)
                .is_some_and(|(nested, _)| nested.marker == fence.marker && nested.len < fence.len)
        });
        let mut same_length_nested_fence_open = false;
        let contains_same_length_nested_fence = body.lines().any(|line| {
            if is_close_fence(line, fence) {
                return same_length_nested_fence_open;
            }
            if parse_open_fence(line)
                .is_some_and(|(nested, _)| nested.marker == fence.marker && nested.len == fence.len)
            {
                same_length_nested_fence_open = true;
            }
            false
        });
        (contains_shorter_nested_fence || contains_same_length_nested_fence).then_some(body)
    }

    if let Some(body) = whole_document_markdown_envelope_body(markdown_source) {
        return Cow::Borrowed(body);
    }

    fn markdown_fence_contains_table(content: &str, is_blockquoted_fence: bool) -> bool {
        let mut previous_line: Option<&str> = None;
        for line in content.lines() {
            let text = if is_blockquoted_fence {
                table_detect::strip_blockquote_prefix(line)
            } else {
                line
            };
            let trimmed = text.trim();
            if trimmed.is_empty() {
                previous_line = None;
                continue;
            }

            if let Some(previous) = previous_line
                && table_detect::is_table_header_line(previous)
                && !table_detect::is_table_delimiter_line(previous)
                && table_detect::is_table_delimiter_line(trimmed)
            {
                return true;
            }

            previous_line = Some(trimmed);
        }
        false
    }

    fn content_from_ranges(source: &str, ranges: &[Range<usize>]) -> String {
        let total_len: usize = ranges.iter().map(ExactSizeIterator::len).sum();
        let mut content = String::with_capacity(total_len);
        for range in ranges {
            content.push_str(&source[range.start..range.end]);
        }
        content
    }

    struct MarkdownCandidateData {
        fence: Fence,
        opening_range: Range<usize>,
        content_ranges: Vec<Range<usize>>,
    }

    // Box the large variant to keep ActiveFence small (~pointer-sized).
    enum ActiveFence {
        Passthrough(Fence),
        MarkdownCandidate(Box<MarkdownCandidateData>),
    }

    let mut out = String::with_capacity(markdown_source.len());
    let mut active_fence: Option<ActiveFence> = None;
    let mut source_offset = 0usize;

    let mut push_source_range = |range: Range<usize>| {
        if !range.is_empty() {
            out.push_str(&markdown_source[range]);
        }
    };

    for line in markdown_source.split_inclusive('\n') {
        let line_start = source_offset;
        source_offset += line.len();
        let line_range = line_start..source_offset;

        if let Some(active) = active_fence.take() {
            match active {
                ActiveFence::Passthrough(fence) => {
                    push_source_range(line_range);
                    if !is_close_fence(line, fence) {
                        active_fence = Some(ActiveFence::Passthrough(fence));
                    }
                }
                ActiveFence::MarkdownCandidate(mut data) => {
                    if is_close_fence(line, data.fence) {
                        if markdown_fence_contains_table(
                            &content_from_ranges(markdown_source, &data.content_ranges),
                            data.fence.is_blockquoted,
                        ) {
                            for range in data.content_ranges {
                                push_source_range(range);
                            }
                        } else {
                            push_source_range(data.opening_range);
                            for range in data.content_ranges {
                                push_source_range(range);
                            }
                            push_source_range(line_range);
                        }
                    } else {
                        data.content_ranges.push(line_range);
                        active_fence = Some(ActiveFence::MarkdownCandidate(data));
                    }
                }
            }
            continue;
        }

        if let Some((fence, is_markdown)) = parse_open_fence(line) {
            if is_markdown {
                active_fence = Some(ActiveFence::MarkdownCandidate(Box::new(
                    MarkdownCandidateData {
                        fence,
                        opening_range: line_range,
                        content_ranges: Vec::new(),
                    },
                )));
            } else {
                push_source_range(line_range);
                active_fence = Some(ActiveFence::Passthrough(fence));
            }
            continue;
        }

        push_source_range(line_range);
    }

    if let Some(active) = active_fence {
        match active {
            ActiveFence::Passthrough(_) => {}
            ActiveFence::MarkdownCandidate(data) => {
                push_source_range(data.opening_range);
                for range in data.content_ranges {
                    push_source_range(range);
                }
            }
        }
    }

    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use ratatui::text::Line;

    fn lines_to_strings(lines: &[Line<'static>]) -> Vec<String> {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.clone())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn citations_render_as_plain_text() {
        let src = "Before 【F:/x.rs†L1】\nAfter 【F:/x.rs†L3】\n";
        let mut out = Vec::new();
        append_markdown(src, /*width*/ None, /*cwd*/ None, &mut out);
        let rendered = lines_to_strings(&out);
        assert_eq!(
            rendered,
            vec![
                "Before 【F:/x.rs†L1】".to_string(),
                "After 【F:/x.rs†L3】".to_string()
            ]
        );
    }

    #[test]
    fn indented_code_blocks_preserve_leading_whitespace() {
        // Basic sanity: indented code with surrounding blank lines should produce the indented line.
        let src = "Before\n\n    code 1\n\nAfter\n";
        let mut out = Vec::new();
        append_markdown(src, /*width*/ None, /*cwd*/ None, &mut out);
        let lines = lines_to_strings(&out);
        assert_eq!(lines, vec!["Before", "", "    code 1", "", "After"]);
    }

    #[test]
    fn append_markdown_preserves_full_text_line() {
        let src = "Hi! How can I help with codex-rs today? Want me to explore the repo, run tests, or work on a specific change?\n";
        let mut out = Vec::new();
        append_markdown(src, /*width*/ None, /*cwd*/ None, &mut out);
        assert_eq!(
            out.len(),
            1,
            "expected a single rendered line for plain text"
        );
        let rendered: String = out
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.clone())
            .collect::<Vec<_>>()
            .join("");
        assert_eq!(
            rendered,
            "Hi! How can I help with codex-rs today? Want me to explore the repo, run tests, or work on a specific change?"
        );
    }

    #[test]
    fn append_markdown_matches_tui_markdown_for_ordered_item() {
        let mut out = Vec::new();
        append_markdown(
            "1. Tight item\n",
            /*width*/ None,
            /*cwd*/ None,
            &mut out,
        );
        let lines = lines_to_strings(&out);
        assert_eq!(lines, vec!["1. Tight item".to_string()]);
    }

    #[test]
    fn append_markdown_keeps_ordered_list_line_unsplit_in_context() {
        let src = "Loose vs. tight list items:\n1. Tight item\n";
        let mut out = Vec::new();
        append_markdown(src, /*width*/ None, /*cwd*/ None, &mut out);

        let lines = lines_to_strings(&out);

        // Expect to find the ordered list line rendered as a single line,
        // not split into a marker-only line followed by the text.
        assert!(
            lines.iter().any(|s| s == "1. Tight item"),
            "expected '1. Tight item' rendered as a single line; got: {lines:?}"
        );
        assert!(
            !lines
                .windows(2)
                .any(|w| w[0].trim_end() == "1." && w[1] == "Tight item"),
            "did not expect a split into ['1.', 'Tight item']; got: {lines:?}"
        );
    }

    #[test]
    fn append_markdown_agent_unwraps_markdown_fences_for_table_rendering() {
        let src = "```markdown\n| A | B |\n|---|---|\n| 1 | 2 |\n```\n";
        let mut out = Vec::new();
        append_markdown_agent(src, /*width*/ None, &mut out);
        let rendered = lines_to_strings(&out);
        assert!(rendered.iter().any(|line| line.contains('━')));
        assert!(rendered.iter().any(|line| line.contains(" 1      2")));
    }

    #[test]
    fn append_markdown_agent_unwraps_markdown_fences_for_no_outer_table_rendering() {
        let src = "```md\nCol A | Col B | Col C\n--- | --- | ---\nx | y | z\n10 | 20 | 30\n```\n";
        let mut out = Vec::new();
        append_markdown_agent(src, /*width*/ None, &mut out);
        let rendered = lines_to_strings(&out);
        assert!(rendered.iter().any(|line| line.contains('━')));
        assert!(
            rendered
                .iter()
                .any(|line| line.contains(" Col A    Col B    Col C"))
        );
        assert!(
            !rendered
                .iter()
                .any(|line| line.trim() == "Col A | Col B | Col C")
        );
    }

    #[test]
    fn append_markdown_agent_unwraps_markdown_fences_for_two_column_no_outer_table() {
        let src = "```md\nA | B\n--- | ---\nleft | right\n```\n";
        let mut out = Vec::new();
        append_markdown_agent(src, /*width*/ None, &mut out);
        let rendered = lines_to_strings(&out);
        assert!(rendered.iter().any(|line| line.contains('━')));
        assert!(rendered.iter().any(|line| line.contains(" left    right")));
        assert!(!rendered.iter().any(|line| line.trim() == "A | B"));
    }

    #[test]
    fn append_markdown_agent_unwraps_markdown_fences_for_single_column_table() {
        let src = "```md\n| Only |\n|---|\n| value |\n```\n";
        let mut out = Vec::new();
        append_markdown_agent(src, /*width*/ None, &mut out);
        let rendered = lines_to_strings(&out);
        assert!(rendered.iter().any(|line| line.contains('━')));
        assert!(!rendered.iter().any(|line| line.trim() == "| Only |"));
    }

    #[test]
    fn append_markdown_agent_keeps_non_markdown_fences_as_code() {
        let src = "```rust\n| A | B |\n|---|---|\n| 1 | 2 |\n```\n";
        let mut out = Vec::new();
        append_markdown_agent(src, /*width*/ None, &mut out);
        let rendered = lines_to_strings(&out);
        assert_eq!(
            rendered,
            vec![
                "| A | B |".to_string(),
                "|---|---|".to_string(),
                "| 1 | 2 |".to_string(),
            ]
        );
    }

    #[test]
    fn append_markdown_agent_unwraps_blockquoted_markdown_fence_table() {
        let src = "> ```markdown\n> | A | B |\n> |---|---|\n> | 1 | 2 |\n> ```\n";
        let rendered = unwrap_markdown_fences(src);
        assert!(
            !rendered.contains("```"),
            "expected markdown fence markers to be removed: {rendered:?}"
        );
    }

    #[test]
    fn append_markdown_agent_keeps_non_blockquoted_markdown_fence_with_blockquote_table_example() {
        let src = "```markdown\n> | A | B |\n> |---|---|\n> | 1 | 2 |\n```\n";
        let normalized = unwrap_markdown_fences(src);
        assert_eq!(normalized, src);
    }

    #[test]
    fn append_markdown_agent_keeps_markdown_fence_when_content_is_not_table() {
        let src = "```markdown\n**bold**\n```\n";
        let mut out = Vec::new();
        append_markdown_agent(src, /*width*/ None, &mut out);
        let rendered = lines_to_strings(&out);
        assert_eq!(rendered, vec!["**bold**".to_string()]);
    }

    #[test]
    fn unwraps_whole_markdown_document_fence_with_nested_code_fences() {
        let body =
            "# Heading 1\n\n```rust\nfn main() {}\n```\n\n```mermaid\nflowchart LR; A-->B\n```\n";
        let src = format!("````markdown\n{body}````\n");

        let normalized = unwrap_markdown_fences(&src);

        assert_eq!(normalized, body);
    }

    #[test]
    fn unwraps_triple_markdown_document_envelope_with_nested_code_fences() {
        let body =
            "# Heading 1\n\n```rust\nfn main() {}\n```\n\n```mermaid\nflowchart LR; A-->B\n```\n";
        let src = format!("```markdown\n{body}```\n");

        let normalized = unwrap_markdown_fences(&src);

        assert_eq!(normalized, body);
    }

    #[test]
    fn unwrap_markdown_fences_repro_keeps_fence_without_header_delimiter_pair() {
        let src = "```markdown\n| A | B |\nnot a delimiter row\n| --- | --- |\n# Heading\n```\n";
        let normalized = unwrap_markdown_fences(src);
        assert_eq!(normalized, src);
    }

    #[test]
    fn append_markdown_agent_keeps_markdown_fence_with_blank_line_between_header_and_delimiter() {
        let src = "```markdown\n| A | B |\n\n|---|---|\n| 1 | 2 |\n```\n";
        let rendered = unwrap_markdown_fences(src);
        assert_eq!(rendered, src);
    }
}
