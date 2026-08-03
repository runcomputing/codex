//! Rasterize Markdown headings for terminals with image support but no text-sizing protocol.

use std::cell::RefCell;

use cosmic_text::Attrs;
use cosmic_text::Buffer;
use cosmic_text::Color;
use cosmic_text::Family;
use cosmic_text::FontSystem;
use cosmic_text::Metrics;
use cosmic_text::Shaping;
use cosmic_text::SwashCache;
use cosmic_text::Weight;
use image::DynamicImage;
use image::Pixel;
use image::Rgba;
use image::RgbaImage;
use ratatui::layout::Size;

use crate::terminal_render::TerminalImage;
use crate::terminal_render::terminal_cell_size;
use crate::width::display_width;

thread_local! {
    static RASTERIZER: RefCell<HeadingRasterizer> = RefCell::new(HeadingRasterizer::new());
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum HeadingRender {
    #[default]
    Plain,
    TextSizing,
    Image,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct HeadingCapabilities {
    pub(super) kitty_text_sizing: bool,
    pub(super) image_rendering: bool,
}

pub(super) fn select_render_mode(capabilities: HeadingCapabilities) -> HeadingRender {
    if capabilities.kitty_text_sizing {
        HeadingRender::TextSizing
    } else if capabilities.image_rendering {
        HeadingRender::Image
    } else {
        HeadingRender::Plain
    }
}

struct HeadingRasterizer {
    font_system: FontSystem,
    swash_cache: SwashCache,
}

impl HeadingRasterizer {
    fn new() -> Self {
        Self {
            font_system: FontSystem::new(),
            swash_cache: SwashCache::new(),
        }
    }

    fn render(&mut self, text: &str, scale: u16, columns: u16) -> Option<DynamicImage> {
        let cell_size = terminal_cell_size();
        let pixel_width = u32::from(columns).checked_mul(u32::from(cell_size.0))?;
        let pixel_height = u32::from(scale).checked_mul(u32::from(cell_size.1))?;
        let font_size = pixel_height as f32 * 0.72;
        let line_height = pixel_height as f32;
        let metrics = Metrics::new(font_size, line_height);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        let mut buffer = buffer.borrow_with(&mut self.font_system);
        buffer.set_size(Some(pixel_width as f32), Some(pixel_height as f32));
        buffer.set_monospace_width(Some(f32::from(cell_size.0) * f32::from(scale)));
        buffer.set_text(
            text,
            &Attrs::new().family(Family::Monospace).weight(Weight::BOLD),
            Shaping::Advanced,
            None,
        );

        let (red, green, blue) = heading_color();
        let mut image = RgbaImage::new(pixel_width, pixel_height);
        let mut drew_pixel = false;
        buffer.draw(
            &mut self.swash_cache,
            Color::rgb(red, green, blue),
            |x, y, width, height, color| {
                let source = Rgba(color.as_rgba());
                for row in 0..height {
                    for column in 0..width {
                        let x = x + column as i32;
                        let y = y + row as i32;
                        if x < 0 || y < 0 {
                            continue;
                        }
                        let (x, y) = (x as u32, y as u32);
                        if x >= pixel_width || y >= pixel_height {
                            continue;
                        }
                        image.get_pixel_mut(x, y).blend(&source);
                        drew_pixel = true;
                    }
                }
            },
        );
        drew_pixel.then_some(DynamicImage::ImageRgba8(image))
    }
}

pub(super) fn render_heading(
    text: &str,
    scale: u16,
    available_columns: usize,
) -> Option<TerminalImage> {
    let columns = display_width(text).checked_mul(usize::from(scale))?;
    if columns == 0 || columns > available_columns {
        return None;
    }
    let columns = u16::try_from(columns).ok()?;
    let image = RASTERIZER.with(|rasterizer| {
        rasterizer
            .try_borrow_mut()
            .ok()?
            .render(text, scale, columns)
    })?;
    TerminalImage::new(image, Size::new(columns, scale))
}

fn heading_color() -> (u8, u8, u8) {
    crate::terminal_palette::default_fg().unwrap_or_else(|| {
        let light_background =
            crate::terminal_palette::default_bg().is_some_and(|(red, green, blue)| {
                299 * u32::from(red) + 587 * u32::from(green) + 114 * u32::from(blue) >= 128_000
            });
        if light_background {
            (31, 31, 31)
        } else {
            (235, 235, 235)
        }
    })
}

#[cfg(test)]
#[path = "heading_tests.rs"]
mod tests;
