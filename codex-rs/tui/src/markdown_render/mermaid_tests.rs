use super::*;

#[test]
fn renders_a_bounded_terminal_image() {
    let rendered = render_mermaid("flowchart LR; A-->B", /*available_columns*/ 40)
        .expect("valid Mermaid should render");

    assert!((1..=40).contains(&rendered.image.size().width));
    assert!((1..=MAX_IMAGE_ROWS).contains(&rendered.image.size().height));
}

#[test]
fn tall_flowchart_scales_to_available_width_without_cropping() {
    let source = "flowchart TD\n    A[Start] --> B{Ready?}\n    B -->|Yes| C[Proceed]\n    B -->|No| D[Prepare]\n    D --> B";
    let theme = mermaid_theme();
    let svg = render_with_options(
        source,
        RenderOptions {
            theme: theme.clone(),
            ..Default::default()
        },
    )
    .expect("Mermaid SVG");
    let output = tempfile::NamedTempFile::new().expect("temporary PNG");
    write_output_png(
        &svg,
        output.path(),
        &RenderConfig {
            background: theme.background.clone(),
            ..Default::default()
        },
        &theme,
    )
    .expect("Mermaid PNG");
    let png = std::fs::read(output.path()).expect("PNG bytes");
    let source_image = image::load_from_memory(&png).expect("decoded PNG");
    let rgba = source_image.to_rgba8();
    let background = *rgba.get_pixel(/*x*/ 0, /*y*/ 0);
    let rightmost_content_column = (0..rgba.width())
        .rev()
        .find(|x| (0..rgba.height()).any(|y| *rgba.get_pixel(*x, y) != background))
        .expect("diagram content");
    let right_padding = rgba.width() - rightmost_content_column - 1;
    assert!(
        right_padding >= 14,
        "Mermaid content has only {right_padding}px of right padding"
    );
    let source_aspect = f64::from(source_image.width()) / f64::from(source_image.height());

    let (resized, bounds) = resize_for_terminal(
        source_image.clone(),
        /*available_columns*/ 160,
        /*available_rows*/ 32,
        (/*width*/ 10, /*height*/ 20),
    )
    .expect("terminal-sized Mermaid image");

    assert!(bounds.width < 160);
    assert!(bounds.height <= 32);
    assert!(resized.width() <= source_image.width() * MAX_ENLARGEMENT);
    assert!(resized.height() <= source_image.height() * MAX_ENLARGEMENT);
    let resized_aspect = f64::from(resized.width()) / f64::from(resized.height());
    assert!((source_aspect - resized_aspect).abs() < 0.01);
}

#[test]
fn image_row_budget_tracks_the_terminal_viewport() {
    assert_eq!(terminal_image_row_budget(Some(48)), 32);
    assert_eq!(terminal_image_row_budget(Some(90)), MAX_IMAGE_ROWS);
    assert_eq!(terminal_image_row_budget(None), DEFAULT_IMAGE_ROWS);
}

#[test]
fn rejects_invalid_or_unbounded_input() {
    assert!(render_mermaid("not a diagram", /*available_columns*/ 40).is_none());
    assert!(render_mermaid("flowchart LR; A-->B", /*available_columns*/ 0).is_none());
    assert!(render_mermaid(&"x".repeat(MAX_SOURCE_BYTES + 1), 40).is_none());
}
