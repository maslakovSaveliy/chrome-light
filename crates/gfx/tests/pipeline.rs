//! End-to-end test: HTML in, pixels out, through the real M1a pipeline
//! (`cl-html` -> `cl-style` -> `cl-layout` -> `cl-paint` -> `cl-gfx`), with no hand-assembled
//! display list anywhere in the chain.
#![allow(
    clippy::expect_used,
    reason = "a failed setup step in a test should abort that test, loudly"
)]

#[path = "common/mod.rs"]
mod common;

use cl_fonts::FontDb;

#[test]
fn page_should_rasterize_end_to_end() {
    let html = r#"<body style="margin:0;background:#00f"><div style="width:10px;height:10px;background:#f00"></div></body>"#;
    let (tree, styled) = common::layout_html(html);
    let display_list = cl_paint::build(&tree, styled.document());

    let fonts = FontDb::bundled().expect("bundled font db");
    let pixmap = cl_gfx::cpu::rasterize(&display_list, 32, 32, &fonts).expect("rasterize");

    let read = |x: u32, y: u32| {
        let p = pixmap
            .pixel(x, y)
            .expect("pixel inside the canvas")
            .demultiply();
        (p.red(), p.green(), p.blue(), p.alpha())
    };

    assert_eq!(read(5, 5), (255, 0, 0, 255), "the div's red background");
    assert_eq!(
        read(20, 20),
        (0, 0, 255, 255),
        "the canvas background propagated from <body>"
    );
}
