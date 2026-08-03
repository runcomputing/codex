use super::*;
use crate::terminal_hyperlinks::visible_lines;
use crate::terminal_render::TerminalLineRender;

#[derive(Debug)]
struct SemanticTailCell {
    lines: Vec<HyperlinkLine>,
}

impl HistoryCell for SemanticTailCell {
    fn display_lines(&self, _width: u16) -> Vec<Line<'static>> {
        visible_lines(self.lines.clone())
    }

    fn display_hyperlink_lines(&self, _width: u16) -> Vec<HyperlinkLine> {
        self.lines.clone()
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        visible_lines(self.lines.clone())
    }
}

#[test]
fn active_tail_renders_terminal_metadata_after_vertical_scroll() {
    let mut heading = HyperlinkLine::new(Line::from("Title     "));
    heading.terminal_render = Some(TerminalLineRender::ScaledText {
        columns: 0..10,
        text: "Title".to_string(),
        scale: 2,
    });
    let cell = SemanticTailCell {
        lines: vec![HyperlinkLine::new(Line::from("older row")), heading],
    };
    let renderable = TranscriptAreaRenderable {
        child: &cell,
        top: 0,
        right: 0,
    };
    let area = Rect::new(0, 0, 20, 1);
    let mut buffer = Buffer::empty(area);

    renderable.render(area, &mut buffer);

    assert_eq!(buffer[(0, 0)].symbol(), "\u{1b}]66;s=2;Title\u{7}");
}
