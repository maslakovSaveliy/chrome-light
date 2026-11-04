//! M1b Task 5: the document pipeline the renderer process and the testshell share. A parsed
//! document lists its subresources before anything is loaded, takes the loaded bytes, and
//! renders at any viewport as often as asked — each render equal to a fresh one.
#![allow(
    clippy::expect_used,
    reason = "a failed setup step in a test should abort that test, loudly"
)]

use std::path::{Path, PathBuf};

use cl_fonts::FontDb;
use cl_net::Url;
use cl_renderer::{LoadedDocument, ParsedDocument, Rendered, StageDumps, SubresourceMap};

/// Both pages of each of the 22 reftest pairs.
const EXPECTED_PAGE_COUNT: usize = 44;

fn reftest_pages() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../testshell/tests/ref");
    let mut pages: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("reftest dir")
        .map(|entry| entry.expect("dir entry").path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("html"))
        .collect();
    pages.sort();
    pages
}

/// Parses `path` and loads every requested subresource from disk, as the testshell does.
fn load(path: &Path) -> LoadedDocument {
    let canonical = path.canonicalize().expect("canonical path");
    let base = Url::from_file_path(&canonical).expect("file url");
    let bytes = std::fs::read(&canonical).expect("read page");
    let parsed = ParsedDocument::parse(&bytes, &base).expect("parse");
    let mut map = SubresourceMap::new();
    for url in parsed.subresource_requests() {
        match cl_net::load_file(url) {
            Ok(bytes) => map.insert_ok(url.clone(), bytes),
            Err(e) => map.insert_err(url.clone(), e.to_string()),
        }
    }
    parsed.attach(map)
}

fn render(doc: &mut LoadedDocument, viewport: (u32, u32)) -> Rendered {
    let mut fonts = FontDb::bundled().expect("fonts");
    doc.render(viewport, &mut fonts, StageDumps::None)
        .expect("render")
}

fn parse_inline(html: &str) -> ParsedDocument {
    let base = Url::parse("file:///dir/page.html").expect("base");
    ParsedDocument::parse(html.as_bytes(), &base).expect("parse")
}

#[test]
fn rerender_should_equal_fresh_render_all_44_pages() {
    let pages = reftest_pages();
    assert_eq!(pages.len(), EXPECTED_PAGE_COUNT);
    let mut changed_by_resize = 0;
    for page in &pages {
        // One FontDb for both renders of the same document, as the renderer process keeps
        // one; the fresh render gets its own.
        let mut fonts = FontDb::bundled().expect("fonts");
        let mut doc = load(page);
        let first = doc
            .render((800, 600), &mut fonts, StageDumps::None)
            .expect("render");
        assert!(first.warnings.is_empty(), "{page:?}: {:?}", first.warnings);
        let resized = doc
            .render((500, 700), &mut fonts, StageDumps::None)
            .expect("render");
        let fresh = render(&mut load(page), (500, 700));
        assert_eq!(resized.display_list, fresh.display_list, "{page:?}");
        assert_eq!(resized.warnings, fresh.warnings, "{page:?}");
        if resized.display_list != first.display_list {
            changed_by_resize += 1;
        }
    }
    // Every list carries its viewport as `bounds`, so a re-render that ignored the new size
    // would differ from the fresh one on every page; this checks the comparison has force.
    assert_eq!(changed_by_resize, EXPECTED_PAGE_COUNT);
}

#[test]
fn rendering_twice_at_one_viewport_should_give_the_same_list() {
    let page = reftest_pages()
        .into_iter()
        .find(|p| p.ends_with("text-basic.html"))
        .expect("text-basic.html");
    let mut doc = load(&page);
    let a = render(&mut doc, (800, 600));
    let b = render(&mut doc, (800, 600));
    assert_eq!(a.display_list, b.display_list);
}

#[test]
fn subresource_requests_should_be_in_document_order_without_duplicates() {
    let parsed = parse_inline(
        r"<!doctype html><link rel=stylesheet href=b.css><link rel=stylesheet href=a.css>
           <link rel=stylesheet href=./b.css><link rel=icon href=i.png><p>x</p>",
    );
    let urls: Vec<&str> = parsed
        .subresource_requests()
        .iter()
        .map(Url::as_str)
        .collect();
    assert_eq!(urls, ["file:///dir/b.css", "file:///dir/a.css"]);
}

#[test]
fn an_attached_stylesheet_should_apply() {
    let html = r#"<!doctype html><link rel=stylesheet href=s.css><body style="margin:0">
                  <div style="height:10px"></div>"#;
    let unstyled = render(
        &mut parse_inline(html).attach(SubresourceMap::new()),
        (100, 100),
    );

    let parsed = parse_inline(html);
    let url = parsed
        .subresource_requests()
        .first()
        .expect("one request")
        .clone();
    let mut map = SubresourceMap::new();
    map.insert_ok(url, b"div { background: #00ff00 }".to_vec());
    let styled = render(&mut parsed.attach(map), (100, 100));

    assert!(styled.warnings.is_empty(), "{:?}", styled.warnings);
    assert_ne!(styled.display_list, unstyled.display_list);
}

#[test]
fn missing_subresource_should_warn_not_fail() {
    let html = r"<!doctype html><link rel=stylesheet href=refused.css>
                  <link rel=stylesheet href=absent.css><p>hi</p>";
    let parsed = parse_inline(html);
    let refused = Url::parse("file:///dir/refused.css").expect("url");
    let mut map = SubresourceMap::new();
    map.insert_err(refused, "outside the document directory".to_owned());
    let rendered = render(&mut parsed.attach(map), (800, 600));

    assert_eq!(rendered.warnings.len(), 2, "{:?}", rendered.warnings);
    let [first, second] = rendered.warnings.as_slice() else {
        unreachable!("length checked above")
    };
    assert_eq!(
        first,
        "file:///dir/refused.css: outside the document directory"
    );
    assert!(second.starts_with("file:///dir/absent.css: "), "{second}");
    assert!(
        !rendered.display_list.items.is_empty(),
        "the page still renders"
    );
}

#[test]
fn stage_dumps_should_be_built_only_when_asked() {
    let mut doc = parse_inline("<!doctype html><p>hi</p>").attach(SubresourceMap::new());
    let mut fonts = FontDb::bundled().expect("fonts");
    let plain = doc
        .render((800, 600), &mut fonts, StageDumps::None)
        .expect("render");
    assert!(plain.stages.is_none());
    let dumped = doc
        .render((800, 600), &mut fonts, StageDumps::All)
        .expect("render");
    let stages = dumped.stages.expect("stages");
    assert!(stages.dom.contains("#document"));
    assert!(stages.fragments.contains("Block"));
    assert_eq!(plain.display_list, dumped.display_list);
}

#[test]
fn an_invalid_viewport_should_be_an_error_not_a_panic() {
    let mut doc = parse_inline("<!doctype html><p>hi</p>").attach(SubresourceMap::new());
    let mut fonts = FontDb::bundled().expect("fonts");
    assert!(doc.render((0, 600), &mut fonts, StageDumps::None).is_err());
    // The document survives a failed render.
    assert!(doc.render((800, 600), &mut fonts, StageDumps::None).is_ok());
}
