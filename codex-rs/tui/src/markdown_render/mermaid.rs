//! Render completed Mermaid fences into bounded PNGs for terminal image protocols.

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

#[derive(Debug)]
pub(super) struct MermaidImage {
    pub(super) image: TerminalImage,
}

pub(super) fn render_mermaid(source: &str, available_columns: usize) -> Option<MermaidImage> {
    if source.is_empty() || source.len() > MAX_SOURCE_BYTES || available_columns == 0 {
        return None;
    }

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
    let columns = u16::try_from(available_columns)
        .unwrap_or(u16::MAX)
        .clamp(1, MAX_IMAGE_COLUMNS);
    let (image, bounds) = resize_for_terminal(
        image,
        columns,
        terminal_image_row_budget(crossterm::terminal::size().ok().map(|(_, rows)| rows)),
        terminal_cell_size(),
    )?;
    let image = TerminalImage::new(image, bounds)?;

    Some(MermaidImage { image })
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
    let width = image.width().div_ceil(u32::from(cell_size.0));
    let height = image.height().div_ceil(u32::from(cell_size.1));
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
