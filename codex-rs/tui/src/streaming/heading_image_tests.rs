//! Terminal-level regression coverage for heading images committed during streaming.

use crate::custom_terminal::Terminal as CustomTerminal;
use crate::history_cell::HistoryRenderMode;
use crate::insert_history::HistoryLineWrapPolicy;
use crate::insert_history::InsertHistoryMode;
use crate::insert_history::insert_history_hyperlink_lines_with_mode_and_wrap_policy;
use crate::markdown_render::MarkdownRenderOptions;
use crate::render::line_utils::line_to_borrowed;
use crate::streaming::controller::StreamController;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::terminal_render::mark_buffer_terminal_rendering;
use crate::test_backend::VT100Backend;
use ratatui::layout::Rect;
use ratatui::layout::Size;
use ratatui::text::Text;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::Wrap;
use std::path::PathBuf;

const WIDTH: u16 = 160;
const HEIGHT: u16 = 48;
const COMPOSER_ROWS: u16 = 3;
const MAX_VIEWPORT_ROWS: u16 = 8;

fn test_cwd() -> PathBuf {
    PathBuf::from("/repo")
}

/// Draw the live tail the way `TranscriptAreaRenderable` does, then place the viewport
/// the way `Tui::draw` does.
fn draw_frame(terminal: &mut CustomTerminal<VT100Backend>, tail: &[HyperlinkLine]) {
    let screen = Size::new(WIDTH, HEIGHT);
    let tail_rows = u16::try_from(tail.len()).unwrap_or(u16::MAX);
    let height = (tail_rows + COMPOSER_ROWS).min(MAX_VIEWPORT_ROWS);
    let mut area = terminal.viewport_area;
    area.height = height;
    area.width = WIDTH;
    area.y = HEIGHT - height;
    if area != terminal.viewport_area {
        let position = terminal.viewport_area.as_position();
        terminal
            .clear_after_position(position)
            .expect("clear for viewport change");
        terminal.set_viewport_area(area);
    }
    terminal
        .draw_with_size(screen, |frame| {
            let area = frame.area();
            let lines = tail.iter().map(|line| line_to_borrowed(&line.line));
            let paragraph =
                Paragraph::new(Text::from(lines.collect::<Vec<_>>())).wrap(Wrap { trim: false });
            let scroll = paragraph
                .line_count(area.width)
                .saturating_sub(usize::from(area.height.saturating_sub(COMPOSER_ROWS)));
            let tail_area = Rect::new(
                area.x,
                area.y,
                area.width,
                area.height.saturating_sub(COMPOSER_ROWS),
            );
            let buf = frame.buffer_mut();
            Clear.render(tail_area, buf);
            paragraph
                .scroll((u16::try_from(scroll).unwrap_or(u16::MAX), 0))
                .render(tail_area, buf);
            mark_buffer_terminal_rendering(buf, tail_area, tail, scroll);
        })
        .expect("draw frame");
}

fn placeholder_rows(terminal: &CustomTerminal<VT100Backend>) -> Vec<usize> {
    let screen = terminal.backend().vt100().screen();
    (0..usize::from(HEIGHT))
        .filter(|row| {
            (0..WIDTH).any(|column| {
                screen
                    .cell(u16::try_from(*row).unwrap_or(0), column)
                    .is_some_and(|cell| cell.contents().contains('\u{10eeee}'))
            })
        })
        .collect()
}

#[test]
fn streaming_image_headings_reach_scrollback_exactly_once() {
    let render_options = MarkdownRenderOptions::image_for_test();
    let mut ctrl = StreamController::new_with_render_options(
        Some(usize::from(WIDTH)),
        &test_cwd(),
        HistoryRenderMode::Rich,
        render_options,
    );
    let backend = VT100Backend::new(WIDTH, HEIGHT);
    let mut terminal = CustomTerminal::with_options(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(
        /*x*/ 0,
        /*y*/ HEIGHT - COMPOSER_ROWS,
        WIDTH,
        COMPOSER_ROWS,
    ));

    let source = concat!(
        "# Closing Thoughts\n\n",
        "Good systems are not merely functional.\n\n",
        "## Final Principle\n\n",
        "> Make the next step obvious.\n\n",
        "## End of Example\n\n",
        "## Keep Exploring\n\n",
        "The best documentation is a map.\n",
    );
    let mut characters = source.chars();
    while let Some(first) = characters.next() {
        let mut delta = first.to_string();
        if let Some(second) = characters.next() {
            delta.push(second);
        }
        ctrl.push(&delta);
        loop {
            let (cell, idle) = ctrl.on_commit_tick();
            if let Some(cell) = cell {
                let lines = cell.display_hyperlink_lines(WIDTH);
                insert_history_hyperlink_lines_with_mode_and_wrap_policy(
                    &mut terminal,
                    &lines,
                    InsertHistoryMode::Standard,
                    HistoryLineWrapPolicy::PreWrap,
                )
                .expect("insert history");
            }
            draw_frame(&mut terminal, &ctrl.current_tail_lines());
            if idle {
                break;
            }
        }
    }

    let rows = placeholder_rows(&terminal);
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for row in &rows {
        match runs.last_mut() {
            Some((_, end)) if *end + 1 == *row => *end = *row,
            _ => runs.push((*row, *row)),
        }
    }
    let screen = terminal.backend().vt100().screen();
    let text = screen
        .rows(/*start*/ 0, WIDTH)
        .enumerate()
        .map(|(row, line)| format!("{row:>2} {line}"))
        .collect::<Vec<_>>()
        .join("\n");

    // One unbroken block of image rows per heading, sized by its level. A heading that reached
    // scrollback twice shows up as an extra run of placeholder rows.
    let heights = runs
        .iter()
        .map(|(start, end)| end - start + 1)
        .collect::<Vec<_>>();
    assert_eq!(heights, vec![6, 5, 5, 5], "runs {runs:?}\n{text}");
}
