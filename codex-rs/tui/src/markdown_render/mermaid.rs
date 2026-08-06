//! Render completed Mermaid fences into bounded PNGs for terminal image protocols.

use std::cell::RefCell;

use mermaid_rs_renderer::RenderConfig;
use mermaid_rs_renderer::RenderOptions;
use mermaid_rs_renderer::Theme;
use mermaid_rs_renderer::render_with_options;
use mermaid_rs_renderer::write_output_png;
use ratatui::layout::Size;
use ratatui_image::FilterType;
use ratatui_image::FontSize;

use crate::terminal_render::TerminalImage;
use crate::terminal_render::terminal_cell_size;

const MAX_SOURCE_BYTES: usize = 64 * 1024;
const MAX_PNG_BYTES: usize = 5 * 1024 * 1024;
const MAX_IMAGE_COLUMNS: u16 = 240;
const DEFAULT_IMAGE_ROWS: u16 = 24;
const MAX_IMAGE_ROWS: u16 = 40;
const MAX_ENLARGEMENT: u32 = 2;
const CACHE_CAPACITY: usize = 32;

#[derive(Debug)]
pub(super) struct MermaidImage {
    pub(super) image: TerminalImage,
}

/// Streaming re-renders call [`render_mermaid`] with the same completed fence many times
/// (stable-prefix renders, fence-close recomputes, finalization). Rendering is a full
/// SVG-layout + PNG + resize pipeline, so results — including failures — are memoized.
#[derive(Clone, Eq, PartialEq)]
struct CacheKey {
    source: String,
    columns: u16,
    row_budget: u16,
    cell_size: FontSize,
}

thread_local! {
    static RENDER_CACHE: RefCell<Vec<(CacheKey, Option<TerminalImage>)>> =
        const { RefCell::new(Vec::new()) };
}

pub(super) fn render_mermaid(source: &str, available_columns: usize) -> Option<MermaidImage> {
    if source.is_empty() || source.len() > MAX_SOURCE_BYTES || available_columns == 0 {
        return None;
    }
    let columns = u16::try_from(available_columns)
        .unwrap_or(u16::MAX)
        .clamp(1, MAX_IMAGE_COLUMNS);
    let row_budget =
        terminal_image_row_budget(crossterm::terminal::size().ok().map(|(_, rows)| rows));
    let cell_size = terminal_cell_size();
    let key = CacheKey {
        source: source.to_string(),
        columns,
        row_budget,
        cell_size,
    };

    let cached = RENDER_CACHE.with(|cache| {
        cache
            .borrow()
            .iter()
            .find(|(entry, _)| *entry == key)
            .map(|(_, image)| image.clone())
    });
    if let Some(image) = cached {
        return image.map(|image| MermaidImage { image });
    }

    let rendered = render_mermaid_uncached(source, columns, row_budget, cell_size);
    RENDER_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() >= CACHE_CAPACITY {
            cache.remove(0);
        }
        cache.push((key, rendered.clone()));
    });
    rendered.map(|image| MermaidImage { image })
}

/// resvg cannot reliably rasterize macOS color-emoji fonts inside the TUI, and one emoji drags
/// its whole text run into the LastResort fallback, rendering every glyph as a boxed
/// placeholder. Strip emoji and their joiners before rendering so label text shapes with real
/// fonts.
fn strip_unrenderable_symbols(source: &str) -> std::borrow::Cow<'_, str> {
    fn unrenderable(character: char) -> bool {
        matches!(character,
            '\u{1F000}'..='\u{1FFFF}' // emoji + supplementary symbol planes
            | '\u{2600}'..='\u{27BF}' // misc symbols and dingbats (checkmarks, gears, …)
            | '\u{2B00}'..='\u{2BFF}' // misc symbols and arrows (stars, …)
            | '\u{FE0E}' | '\u{FE0F}' // variation selectors
            | '\u{200D}'              // zero-width joiner
            | '\u{20E3}'              // combining enclosing keycap
        )
    }
    if !source.chars().any(unrenderable) {
        return std::borrow::Cow::Borrowed(source);
    }
    let mut out = String::with_capacity(source.len());
    let mut swallow_following_space = false;
    for character in source.chars() {
        if unrenderable(character) {
            swallow_following_space = true;
            continue;
        }
        if swallow_following_space && character == ' ' {
            swallow_following_space = false;
            continue;
        }
        swallow_following_space = false;
        out.push(character);
    }
    std::borrow::Cow::Owned(out)
}

fn render_mermaid_uncached(
    source: &str,
    columns: u16,
    row_budget: u16,
    cell_size: FontSize,
) -> Option<TerminalImage> {
    let source = strip_unrenderable_symbols(source);
    let source = source.as_ref();
    let theme = mermaid_theme();
    let options = RenderOptions {
        theme: theme.clone(),
        ..Default::default()
    };
    let svg = render_with_options(source, options).ok()?;
    let output = tempfile::NamedTempFile::new().ok()?;
    let render_config = RenderConfig {
        background: theme.background.clone(),
        ..Default::default()
    };
    write_output_png(&svg, output.path(), &render_config, &theme).ok()?;
    let png = std::fs::read(output.path()).ok()?;
    if png.len() > MAX_PNG_BYTES {
        return None;
    }

    let image = image::load_from_memory(&png).ok()?;
    let (image, bounds) = resize_for_terminal(image, columns, row_budget, cell_size)?;
    TerminalImage::new(image, bounds)
}

fn resize_for_terminal(
    image: image::DynamicImage,
    available_columns: u16,
    available_rows: u16,
    cell_size: FontSize,
) -> Option<(image::DynamicImage, Size)> {
    let terminal_width = u32::from(available_columns).checked_mul(u32::from(cell_size.0))?;
    let terminal_height = u32::from(available_rows).checked_mul(u32::from(cell_size.1))?;
    let max_width = terminal_width.min(image.width().saturating_mul(MAX_ENLARGEMENT));
    let max_height = terminal_height.min(image.height().saturating_mul(MAX_ENLARGEMENT));
    if max_width == 0 || max_height == 0 {
        return None;
    }

    // `ratatui_image::Resize::Fit` deliberately does not enlarge images. Mermaid diagrams are
    // commonly smaller than a modern HiDPI terminal viewport, so enlarge the decoded PNG to its
    // pixel budget first. `DynamicImage::resize` fits inside both limits without cropping and
    // preserves the source aspect ratio.
    let image = image.resize(max_width, max_height, FilterType::Lanczos3);
    let cell_width = u32::from(cell_size.0);
    let cell_height = u32::from(cell_size.1);
    let width = image.width().div_ceil(cell_width);
    let height = image.height().div_ceil(cell_height);
    // Pad to an exact cell grid: protocol creation skips its own (second) Lanczos3 resample when
    // an axis already matches the cell footprint, so padding here avoids resizing twice.
    let canvas_width = width.checked_mul(cell_width)?;
    let canvas_height = height.checked_mul(cell_height)?;
    let image = if canvas_width == image.width() || canvas_height == image.height() {
        image
    } else {
        let mut canvas = image::RgbaImage::new(canvas_width, canvas_height);
        image::imageops::overlay(&mut canvas, &image.to_rgba8(), /*x*/ 0, /*y*/ 0);
        image::DynamicImage::ImageRgba8(canvas)
    };
    let bounds = Size::new(u16::try_from(width).ok()?, u16::try_from(height).ok()?);
    Some((image, bounds))
}

fn terminal_image_row_budget(terminal_rows: Option<u16>) -> u16 {
    terminal_rows.map_or(DEFAULT_IMAGE_ROWS, |rows| {
        (rows.saturating_mul(2) / 3).clamp(1, MAX_IMAGE_ROWS)
    })
}

fn mermaid_theme() -> Theme {
    let is_dark = crate::terminal_palette::default_bg().is_none_or(|(red, green, blue)| {
        let luminance = 299 * u32::from(red) + 587 * u32::from(green) + 114 * u32::from(blue);
        luminance < 128_000
    });
    if is_dark {
        Theme::dark()
    } else {
        Theme::modern()
    }
}

#[cfg(test)]
#[path = "mermaid_tests.rs"]
mod tests;
