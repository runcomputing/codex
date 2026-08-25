use image::DynamicImage;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;

use super::*;

#[test]
fn terminal_cell_dimensions_do_not_exceed_non_divisible_window_pixels() {
    let window = crossterm::terminal::WindowSize {
        columns: 62,
        rows: 32,
        width: 643,
        height: 701,
    };

    assert_eq!(terminal_cell_dimensions(&window), Some((10, 21)));
}

fn test_image(columns: u16, rows: u16) -> DynamicImage {
    let (cell_width, cell_height) = terminal_cell_size();
    DynamicImage::new_rgba8(
        u32::from(cell_width) * u32::from(columns),
        u32::from(cell_height) * u32::from(rows),
    )
}

#[test]
fn marks_scaled_text_and_reserved_cells() {
    let mut scaled = HyperlinkLine::new(Line::from("Title     "));
    scaled.terminal_render = Some(TerminalLineRender::ScaledText {
        columns: 0..10,
        text: "Title".to_string(),
        scale: 2,
    });
    let mut reserved = HyperlinkLine::new(Line::from("          "));
    reserved.terminal_render = Some(TerminalLineRender::Reserved { columns: 0..10 });
    let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 2));

    mark_buffer_terminal_rendering(
        &mut buffer,
        Rect::new(0, 0, 12, 2),
        &[scaled, reserved],
        /*scroll_rows*/ 0,
    );

    assert_eq!(buffer[(0, 0)].symbol(), "\x1b]66;s=2;Title\x07");
    assert!(buffer[(1, 0)].skip);
    assert!(buffer[(0, 1)].skip);
}

#[test]
fn ratatui_image_uses_kitty_placeholders_and_fixed_footprint() {
    let image = TerminalImage::new(
        test_image(/*columns*/ 4, /*rows*/ 3),
        Size::new(/*width*/ 4, /*height*/ 3),
    )
    .expect("terminal image");
    assert_eq!(image.size(), Size::new(/*width*/ 4, /*height*/ 3));
    assert!(
        image
            .row_symbol(/*row*/ 1)
            .expect("row")
            .contains("a=T,U=1,f=32,t=d"),
        "{:?}",
        image.row_symbol(/*row*/ 1)
    );
    let mut line = HyperlinkLine::new(Line::from("    "));
    line.terminal_render = Some(TerminalLineRender::Image {
        columns: 0..4,
        image,
    });
    let mut buffer = Buffer::empty(Rect::new(0, 0, 6, 3));

    mark_buffer_terminal_rendering(
        &mut buffer,
        Rect::new(0, 0, 6, 3),
        &[line],
        /*scroll_rows*/ 0,
    );

    assert!(buffer[(0, 0)].symbol().contains("a=T,U=1,f=32,t=d"));
    assert!(buffer[(1, 0)].skip);
    assert!(buffer[(1, 1)].skip);
}

#[test]
fn sliced_image_renders_when_its_first_row_is_scrolled_offscreen() {
    let image = TerminalImage::new(
        test_image(/*columns*/ 4, /*rows*/ 3),
        Size::new(/*width*/ 4, /*height*/ 3),
    )
    .expect("terminal image");
    assert_eq!(image.size(), Size::new(/*width*/ 4, /*height*/ 3));
    assert!(
        image
            .row_symbol(/*row*/ 1)
            .expect("image row")
            .contains("a=T,U=1,f=32,t=d"),
        "{:?}",
        image.row_symbol(/*row*/ 1)
    );
    let mut lines = Vec::new();
    let mut first = HyperlinkLine::new(Line::from("    "));
    first.terminal_render = Some(TerminalLineRender::Image {
        columns: 0..4,
        image: image.clone(),
    });
    lines.push(first);
    for row in 1..3 {
        let mut continuation = HyperlinkLine::new(Line::from("    "));
        continuation.terminal_render = Some(TerminalLineRender::ImageRow {
            columns: 0..4,
            image: image.clone(),
            row,
        });
        lines.push(continuation);
    }
    let mut buffer = Buffer::empty(Rect::new(0, 0, 6, 2));

    mark_buffer_terminal_rendering(
        &mut buffer,
        Rect::new(0, 0, 6, 2),
        &lines,
        /*scroll_rows*/ 1,
    );

    assert!(
        buffer[(0, 0)].symbol().contains("a=T,U=1,f=32,t=d"),
        "{:?}",
        buffer[(0, 0)].symbol()
    );
    assert!(buffer[(1, 0)].skip);
    assert!(buffer[(1, 1)].skip);
}

#[test]
fn scrollback_image_row_symbol_advances_the_terminal_cursor() {
    let image = TerminalImage::new(
        test_image(/*columns*/ 4, /*rows*/ 3),
        Size::new(/*width*/ 4, /*height*/ 3),
    )
    .expect("terminal image");

    let symbol = image.row_symbol(/*row*/ 1).expect("image row");

    assert!(symbol.contains("\x1b[0B"));
    let mut terminal =
        vt100::Parser::new(/*rows*/ 4, /*cols*/ 8, /*scrollback_len*/ 0);
    terminal.process(b"\x1b[2;1H");
    terminal.process(symbol.as_bytes());
    assert_eq!(terminal.screen().cursor_position(), (2, 7));
}
