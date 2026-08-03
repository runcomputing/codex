use pretty_assertions::assert_eq;

use super::*;

#[test]
fn selects_kitty_text_sizing_and_ghostty_image_rendering() {
    assert_eq!(
        select_render_mode(HeadingCapabilities {
            kitty_text_sizing: true,
            image_rendering: true,
        }),
        HeadingRender::TextSizing
    );
    assert_eq!(
        select_render_mode(HeadingCapabilities {
            kitty_text_sizing: false,
            image_rendering: true,
        }),
        HeadingRender::Image
    );
    assert_eq!(
        select_render_mode(HeadingCapabilities {
            kitty_text_sizing: false,
            image_rendering: false,
        }),
        HeadingRender::Plain
    );
}

#[test]
fn rasterized_heading_uses_requested_cell_footprint() {
    let image =
        render_heading("Title", /*scale*/ 2, /*available_columns*/ 40).expect("rasterized heading");

    assert_eq!(image.size(), Size::new(/*width*/ 10, /*height*/ 2));
}

#[test]
fn rasterized_heading_falls_back_when_it_cannot_fit() {
    assert_eq!(
        render_heading(
            "Long title",
            /*scale*/ 3,
            /*available_columns*/ 10
        ),
        None
    );
}

#[test]
fn rasterized_heading_has_no_clipped_or_blank_pixel_rows() {
    use image::GenericImageView;

    let mut rasterizer = HeadingRasterizer::new();
    for scale in [2, 3, 4, 5, 6] {
        let image = rasterizer
            .render("Heading 1", scale, 9 * scale)
            .expect("heading image");
        let (width, height) = image.dimensions();
        let rgba = image.to_rgba8();
        let occupied_rows = (0..height)
            .filter(|y| (0..width).any(|x| rgba.get_pixel(x, *y)[3] != 0))
            .collect::<Vec<_>>();
        let first_row = *occupied_rows.first().expect("non-empty heading raster");
        let last_row = *occupied_rows.last().expect("non-empty heading raster");

        assert!(first_row > 0, "scale {scale} raster is clipped at the top");
        assert!(
            last_row < height - 1,
            "scale {scale} raster is clipped at the bottom"
        );
        assert_eq!(
            occupied_rows,
            (first_row..=last_row).collect::<Vec<_>>(),
            "scale {scale} raster contains a blank horizontal seam"
        );
    }
}
