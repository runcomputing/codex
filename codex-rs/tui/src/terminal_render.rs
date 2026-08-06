//! Semantic terminal rendering carried separately from visible TUI text.
//!
//! Ratatui must lay out a feature's cell footprint before its escape sequence reaches the
//! terminal. Rich headings and inline images therefore retain placeholder cells while this module
//! replaces that footprint at the final buffer or scrollback-writing boundary.

use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use image::DynamicImage;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::layout::Size;
use ratatui::text::Span;
use ratatui::text::Text;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::Wrap;
use ratatui_image::FilterType;
use ratatui_image::FontSize;
use ratatui_image::Image as RatatuiImage;
use ratatui_image::Resize;
use ratatui_image::picker::Picker;
use ratatui_image::picker::ProtocolType;

use crate::render::line_utils::line_to_borrowed;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::width::char_width;

const FALLBACK_CELL_WIDTH_PX: u16 = 10;
const FALLBACK_CELL_HEIGHT_PX: u16 = 20;

#[derive(Clone)]
pub(crate) struct TerminalImage {
    inner: Arc<TerminalImageInner>,
}

struct TerminalImageInner {
    size: Size,
    row_symbols: Vec<String>,
    transmit_prefix: Option<String>,
}

impl TerminalImage {
    pub(crate) fn new(image: DynamicImage, bounds: Size) -> Option<Self> {
        if bounds.width == 0 || bounds.height == 0 {
            return None;
        }
        #[allow(deprecated)]
        let mut picker = Picker::from_fontsize(terminal_cell_size());
        picker.set_protocol_type(ProtocolType::Kitty);
        let protocol = picker
            .new_protocol(
                image,
                Rect::new(/*x*/ 0, /*y*/ 0, bounds.width, bounds.height),
                Resize::Fit(Some(FilterType::Lanczos3)),
            )
            .ok()?;
        let area = protocol.area();
        let size = Size::new(area.width, area.height);
        if size.width == 0 || size.height == 0 {
            return None;
        }
        let mut buffer = Buffer::empty(Rect::new(
            /*x*/ 0,
            /*y*/ 0,
            size.width,
            size.height,
        ));
        RatatuiImage::new(&protocol).render(area, &mut buffer);
        let mut row_symbols = (0..size.height)
            .map(|row| normalize_row_symbol(buffer[(0, row)].symbol(), size.height))
            .collect::<Vec<_>>();
        let transmit_prefix = row_symbols
            .first()
            .and_then(|symbol| {
                symbol
                    .find("\x1b[s")
                    .map(|index| symbol[..index].to_string())
            })
            .filter(|prefix| !prefix.is_empty());
        if let Some(template) = row_symbols.first().map(|symbol| {
            transmit_prefix
                .as_ref()
                .and_then(|prefix| symbol.strip_prefix(prefix))
                .unwrap_or(symbol)
                .to_string()
        }) {
            for row in 1..size.height {
                let index = usize::from(row);
                if row_symbols[index].trim().is_empty() {
                    row_symbols[index] = synthesize_kitty_row_symbol(&template, row);
                }
            }
        }
        Some(Self {
            inner: Arc::new(TerminalImageInner {
                size,
                row_symbols,
                transmit_prefix,
            }),
        })
    }

    pub(crate) fn size(&self) -> Size {
        self.inner.size
    }

    fn row_symbol(&self, row: u16) -> Option<String> {
        self.row_symbol_with_transmit(row, true)
    }

    fn row_symbol_with_transmit(&self, row: u16, include_transmit: bool) -> Option<String> {
        let size = self.size();
        if row >= size.height {
            return None;
        }
        let mut symbol = self.inner.row_symbols[usize::from(row)].clone();
        if include_transmit
            && let Some(prefix) = &self.inner.transmit_prefix
            && !symbol.starts_with(prefix)
        {
            symbol.insert_str(0, prefix);
        }
        Some(symbol)
    }
}

fn normalize_row_symbol(symbol: &str, image_height: u16) -> String {
    let mut symbol = symbol.to_string();
    let full_height_down = image_height.saturating_sub(1);
    let suffix = format!("\x1b[{full_height_down}B");
    if symbol.ends_with(&suffix) {
        symbol.truncate(symbol.len() - suffix.len());
        symbol.push_str("\x1b[0B");
    }
    symbol
}

fn synthesize_kitty_row_symbol(template: &str, row: u16) -> String {
    let Some(marker_index) = template.find('\u{10EEEE}') else {
        return template.to_string();
    };
    let row_diacritic_start = marker_index + '\u{10EEEE}'.len_utf8();
    let Some((_, row_diacritic)) = template[row_diacritic_start..].char_indices().next() else {
        return template.to_string();
    };
    let row_diacritic_end = row_diacritic_start + row_diacritic.len_utf8();
    let mut symbol = String::with_capacity(template.len());
    symbol.push_str(&template[..row_diacritic_start]);
    symbol.push(kitty_row_diacritic(row));
    symbol.push_str(&template[row_diacritic_end..]);
    symbol
}

fn kitty_row_diacritic(row: u16) -> char {
    const ROW_DIACRITICS: [char; 40] = [
        '\u{305}', '\u{30D}', '\u{30E}', '\u{310}', '\u{312}', '\u{33D}', '\u{33E}', '\u{33F}',
        '\u{346}', '\u{34A}', '\u{34B}', '\u{34C}', '\u{350}', '\u{351}', '\u{352}', '\u{357}',
        '\u{35B}', '\u{363}', '\u{364}', '\u{365}', '\u{366}', '\u{367}', '\u{368}', '\u{369}',
        '\u{36A}', '\u{36B}', '\u{36C}', '\u{36D}', '\u{36E}', '\u{36F}', '\u{483}', '\u{484}',
        '\u{485}', '\u{486}', '\u{487}', '\u{592}', '\u{593}', '\u{594}', '\u{595}', '\u{597}',
    ];
    ROW_DIACRITICS
        .get(usize::from(row))
        .copied()
        .unwrap_or(ROW_DIACRITICS[0])
}

impl fmt::Debug for TerminalImage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TerminalImage")
            .field("size", &self.size())
            .finish_non_exhaustive()
    }
}

impl PartialEq for TerminalImage {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for TerminalImage {}

pub(crate) fn terminal_cell_size() -> FontSize {
    crossterm::terminal::window_size()
        .ok()
        .as_ref()
        .and_then(terminal_cell_dimensions)
        .unwrap_or((FALLBACK_CELL_WIDTH_PX, FALLBACK_CELL_HEIGHT_PX))
}

fn terminal_cell_dimensions(window: &crossterm::terminal::WindowSize) -> Option<(u16, u16)> {
    (window.columns > 0 && window.rows > 0 && window.width > 0 && window.height > 0).then(|| {
        (
            (window.width / window.columns).max(1),
            (window.height / window.rows).max(1),
        )
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TerminalLineRender {
    ScaledText {
        columns: Range<usize>,
        text: String,
        scale: u16,
    },
    Image {
        columns: Range<usize>,
        image: TerminalImage,
    },
    ImageRow {
        columns: Range<usize>,
        image: TerminalImage,
        row: u16,
    },
    Reserved {
        columns: Range<usize>,
    },
}

impl TerminalLineRender {
    pub(crate) fn shifted(&self, shift: usize) -> Self {
        match self {
            Self::ScaledText {
                columns,
                text,
                scale,
            } => Self::ScaledText {
                columns: columns.start + shift..columns.end + shift,
                text: text.clone(),
                scale: *scale,
            },
            Self::Image { columns, image } => Self::Image {
                columns: columns.start + shift..columns.end + shift,
                image: image.clone(),
            },
            Self::ImageRow {
                columns,
                image,
                row,
            } => Self::ImageRow {
                columns: columns.start + shift..columns.end + shift,
                image: image.clone(),
                row: *row,
            },
            Self::Reserved { columns } => Self::Reserved {
                columns: columns.start + shift..columns.end + shift,
            },
        }
    }

    pub(crate) fn is_reserved(&self) -> bool {
        matches!(self, Self::Reserved { .. })
    }
}

pub(crate) fn osc66_text(text: &str, scale: u16) -> String {
    let safe_text = text
        .chars()
        .filter(|character| !character.is_control())
        .collect::<String>();
    // Leave `w` at its default of zero so the terminal computes the text width. The protocol only
    // permits explicit widths from 0 through 7, which is too small for general heading text.
    format!("\x1b]66;s={scale};{safe_text}\x07")
}

pub(crate) fn decorate_terminal_spans(line: &HyperlinkLine) -> Option<Vec<Span<'static>>> {
    match line.terminal_render.as_ref()? {
        TerminalLineRender::ScaledText {
            columns,
            text,
            scale,
        } => {
            let (mut spans, style) = spans_before_column(line, columns.start);
            spans.push(Span::styled(osc66_text(text, *scale), style));
            Some(spans)
        }
        TerminalLineRender::Image { columns, image } => {
            let (mut spans, style) = spans_before_column(line, columns.start);
            spans.push(Span::styled(image.row_symbol(/*row*/ 0)?, style));
            Some(spans)
        }
        TerminalLineRender::ImageRow {
            columns,
            image,
            row,
        } => {
            let (mut spans, style) = spans_before_column(line, columns.start);
            spans.push(Span::styled(
                image.row_symbol_with_transmit(*row, false)?,
                style,
            ));
            Some(spans)
        }
        TerminalLineRender::Reserved { .. } => Some(Vec::new()),
    }
}

fn spans_before_column(
    line: &HyperlinkLine,
    target: usize,
) -> (Vec<Span<'static>>, ratatui::style::Style) {
    let mut spans = Vec::new();
    let mut column = 0usize;
    let mut terminal_style = line.line.style;
    for span in &line.line.spans {
        for character in span.content.chars() {
            let width = if character.is_control() {
                0
            } else {
                char_width(character)
            };
            if column < target {
                push_styled_content(&mut spans, &character.to_string(), span.style);
            } else if column == target {
                terminal_style = span.style;
            }
            column += width;
            if column >= target {
                break;
            }
        }
        if column >= target {
            break;
        }
    }
    (spans, terminal_style)
}

fn push_styled_content(out: &mut Vec<Span<'static>>, content: &str, style: ratatui::style::Style) {
    if let Some(last) = out.last_mut()
        && last.style == style
    {
        last.content.to_mut().push_str(content);
        return;
    }
    out.push(Span::styled(content.to_string(), style));
}

pub(crate) fn mark_buffer_terminal_rendering(
    buf: &mut Buffer,
    area: Rect,
    lines: &[HyperlinkLine],
    scroll_rows: usize,
) {
    // This runs on every frame: skip the per-line wrap layout entirely for the common case of a
    // cell with no terminal-rendered content, and stop once past the visible band.
    if area.width == 0 || lines.iter().all(|line| line.terminal_render.is_none()) {
        return;
    }

    let visible_end = scroll_rows.saturating_add(usize::from(area.height));
    let mut logical_row = 0usize;
    let mut rendered_image: Option<(usize, TerminalImage)> = None;
    for line in lines {
        if logical_row >= visible_end {
            break;
        }
        let paragraph =
            Paragraph::new(Text::from(line_to_borrowed(&line.line))).wrap(Wrap { trim: false });
        let rendered_height = paragraph.line_count(area.width).max(/*other*/ 1);
        let Some(terminal_render) = &line.terminal_render else {
            logical_row += rendered_height;
            continue;
        };
        if logical_row >= scroll_rows && logical_row - scroll_rows < usize::from(area.height) {
            let y = area.y + (logical_row - scroll_rows) as u16;
            match terminal_render {
                TerminalLineRender::ScaledText {
                    columns,
                    text,
                    scale,
                } => {
                    if let Some((x, _width)) = terminal_cell_range(area, columns) {
                        buf[(x, y)].set_symbol(&osc66_text(text, *scale));
                        mark_reserved_cells(buf, area, y, columns.start + 1..columns.end);
                    }
                }
                TerminalLineRender::Image { columns, image } => {
                    render_image(buf, area, columns, image, logical_row, scroll_rows);
                    rendered_image = Some((logical_row, image.clone()));
                }
                TerminalLineRender::ImageRow {
                    columns,
                    image,
                    row,
                } => {
                    let image_top = logical_row.saturating_sub(usize::from(*row));
                    if !rendered_image
                        .as_ref()
                        .is_some_and(|(top, rendered)| *top == image_top && rendered == image)
                    {
                        render_image(buf, area, columns, image, image_top, scroll_rows);
                        rendered_image = Some((image_top, image.clone()));
                    }
                }
                TerminalLineRender::Reserved { columns } => {
                    mark_reserved_cells(buf, area, y, columns.clone());
                }
            }
        }
        logical_row += rendered_height;
    }
}

fn render_image(
    buf: &mut Buffer,
    area: Rect,
    columns: &Range<usize>,
    image: &TerminalImage,
    image_top: usize,
    scroll_rows: usize,
) {
    let Some((x, width)) = terminal_cell_range(area, columns) else {
        return;
    };
    let height = usize::from(image.size().height);
    let mut transmitted = false;
    for screen_row in 0..area.height {
        let logical_row = scroll_rows + usize::from(screen_row);
        if logical_row < image_top {
            continue;
        }
        let image_row = logical_row - image_top;
        if image_row >= height {
            break;
        }
        let Some(symbol) = u16::try_from(image_row).ok().and_then(|row| {
            let symbol = image.row_symbol_with_transmit(row, !transmitted);
            transmitted = true;
            symbol
        }) else {
            continue;
        };
        let y = area.y + screen_row;
        buf[(x, y)].set_symbol(&symbol);
        mark_reserved_cells(
            buf,
            area,
            y,
            columns.start + 1..columns.start + usize::from(width),
        );
    }
}

fn terminal_cell_range(area: Rect, columns: &Range<usize>) -> Option<(u16, u16)> {
    let footprint = columns.end.saturating_sub(columns.start);
    let start = u16::try_from(columns.start)
        .ok()
        .filter(|start| *start < area.width)?;
    let available = usize::from(area.width - start);
    let width = u16::try_from(footprint.min(available)).ok()?;
    if width == 0 {
        return None;
    }
    Some((area.x + start, width))
}

fn mark_reserved_cells(buf: &mut Buffer, area: Rect, y: u16, columns: Range<usize>) {
    for column in columns {
        let Ok(column) = u16::try_from(column) else {
            break;
        };
        if column >= area.width {
            break;
        }
        buf[(area.x + column, y)].set_symbol(" ").set_skip(true);
    }
}

#[cfg(test)]
#[path = "terminal_render_tests.rs"]
mod tests;
