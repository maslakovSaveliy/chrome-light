//! The renderer process (M1b Task 8): document bytes from the browser → display lists for the
//! gpu, over [`cl_renderer`]. [`RendererState`] is the whole policy as a pure state machine;
//! `child::serve_renderer` is its IO shell (channels, shared memory, the clock).
//!
//! Contract with the browser (`cl_ipc::proto::renderer::ToRenderer` docs):
//!
//! * Every `seq` the browser hands over — `LoadDocument`, `Resize`, `Redraw` — is answered
//!   with a display list for it, laid out for the latest viewport, or with `RenderFailed` for
//!   it. Before the document is loaded, a `Resize`/`Redraw` only sets the size and `seq` of the
//!   first list; a failed or outdated layout is redone, never resent.
//! * One `NeedSubresources` per document, within the protocol limits (an over-limit message
//!   would get this process killed): at most [`MAX_SUBRESOURCES_PER_DOCUMENT`] URLs, each at
//!   most [`MAX_URL_BYTES`]; the rest render as missing. The answer is awaited for at most
//!   [`SUBRESOURCE_TIMEOUT`]; then the document renders with a warning per unanswered URL.
//! * Warnings are cut to [`MAX_WARNINGS`] × [`MAX_WARNING_BYTES`].
//! * A failed display-list send means the gpu is gone: the list is kept and sent again when a
//!   `DisplayListSender` of a newer generation arrives (the browser also asks for a `Redraw`).
//!   Every newer generation gets the last list, even one a dying gpu accepted.
//! * `DocumentReady` goes out when the list is made; the list itself may still be waiting for
//!   a sender (none yet, or the current one failed).

use std::time::{Duration, Instant};

use cl_fonts::{FontDb, FontError};
use cl_ipc::proto::renderer::{FromRenderer, RenderFailureKind, SubresourceRefusal};
use cl_ipc::proto::{
    MAX_SUBRESOURCES_PER_DOCUMENT, MAX_URL_BYTES, MAX_WARNING_BYTES, MAX_WARNINGS, Viewport,
};
use cl_net::Url;
use cl_renderer::{LoadedDocument, ParsedDocument, RenderError, StageDumps, SubresourceMap};

/// How long the renderer waits for the browser's subresource answer before it renders without.
pub const SUBRESOURCE_TIMEOUT: Duration = Duration::from_secs(5);

/// What happened, as plain data: the IO shell copies the browser's shared memory and keeps the
/// display-list channel itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RendererInput {
    /// `ToRenderer::LoadDocument`.
    Load {
        /// The document.
        doc_id: u64,
        /// Its URL.
        base_url: String,
        /// The surface to lay out for.
        viewport: Viewport,
        /// The frame number to answer.
        seq: u64,
        /// The document bytes.
        bytes: Vec<u8>,
    },
    /// `ToRenderer::Subresources`: `(index into the request, bytes or refusal)`.
    Subresources {
        /// The document.
        doc_id: u64,
        /// The answers.
        replies: Vec<(u32, Result<Vec<u8>, SubresourceRefusal>)>,
    },
    /// `ToRenderer::Resize`.
    Resize {
        /// The new surface size.
        viewport: Viewport,
        /// The frame number to answer.
        seq: u64,
    },
    /// `ToRenderer::Redraw`.
    Redraw {
        /// The frame number to answer.
        seq: u64,
    },
    /// `ToRenderer::DisplayListSender`: the shell now holds this gpu generation's sender.
    DisplayListSender {
        /// The gpu generation.
        generation: u64,
    },
    /// Sending a display list on the current sender failed: that gpu is gone.
    DisplayListSendFailed,
    /// `ToRenderer::Ping`.
    Ping(u64),
    /// `ToRenderer::Shutdown`.
    Shutdown,
    /// Time passed.
    Tick,
}

/// What the IO shell must do, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RendererOutput {
    /// Send to the browser.
    Up(FromRenderer),
    /// Send this encoded display list to the gpu on the current sender.
    SendDisplayList {
        /// Its frame number.
        seq: u64,
        /// `cl_paint::wire` bytes.
        bytes: Vec<u8>,
    },
    /// Exit the process.
    Exit,
}

/// Where the document is.
enum Doc {
    /// No `LoadDocument` yet.
    None,
    /// Waiting for the subresource answer until `deadline`.
    Awaiting {
        doc_id: u64,
        parsed: ParsedDocument,
        /// The URLs asked for, by index of the request.
        requested: Vec<Url>,
        deadline: Instant,
    },
    Loaded(LoadedDocument),
    /// The document cannot be rendered at all (its base URL or bytes did not parse).
    Failed,
}

/// The last display list made, for a `Redraw` at the same size and for resending.
struct LastList {
    seq: u64,
    viewport: Viewport,
    bytes: Vec<u8>,
    warnings: Vec<String>,
    /// Handed to a sender that has not failed since.
    delivered: bool,
}

/// The renderer's state between inputs. See the module docs for the contract.
pub struct RendererState {
    fonts: FontDb,
    max_list_bytes: usize,
    doc: Doc,
    /// The latest surface size the browser asked for.
    viewport: Option<Viewport>,
    /// The latest `seq` the browser asked for.
    seq: u64,
    last: Option<LastList>,
    /// The generation of the sender the shell holds, while it works.
    sender: Option<u64>,
    /// The newest generation seen, working or not.
    newest_generation: u64,
}

impl RendererState {
    /// A renderer with the bundled fonts whose display lists may be at most `max_list_bytes`
    /// encoded (production: `cl_paint::wire::MAX_DISPLAY_LIST_BYTES`, what the gpu accepts).
    ///
    /// # Errors
    /// [`FontError`] if the bundled fonts do not load (a corrupted build).
    pub fn new(max_list_bytes: usize) -> Result<Self, FontError> {
        Ok(RendererState {
            fonts: FontDb::bundled()?,
            max_list_bytes,
            doc: Doc::None,
            viewport: None,
            seq: 0,
            last: None,
            sender: None,
            newest_generation: 0,
        })
    }

    /// Handles one input at `now` and returns what to do, in order.
    pub fn on(&mut self, input: RendererInput, now: Instant) -> Vec<RendererOutput> {
        let mut out = Vec::new();
        match input {
            RendererInput::Load {
                doc_id,
                base_url,
                viewport,
                seq,
                bytes,
            } => {
                self.viewport = Some(viewport);
                self.seq = seq;
                self.last = None;
                self.load(doc_id, &base_url, &bytes, now, &mut out);
            }
            RendererInput::Subresources { doc_id, replies } => {
                self.on_subresources(doc_id, replies, &mut out);
            }
            RendererInput::Resize { viewport, seq } => {
                self.viewport = Some(viewport);
                self.seq = seq;
                self.answer(&mut out);
            }
            RendererInput::Redraw { seq } => {
                self.seq = seq;
                self.redraw(&mut out);
            }
            RendererInput::DisplayListSender { generation } => {
                if generation > self.newest_generation {
                    self.newest_generation = generation;
                    self.sender = Some(generation);
                    // A new gpu has never seen the list: the last send may have succeeded into
                    // a gpu the browser was already killing. At worst the new gpu gets one
                    // stale list, which it reports as Stale.
                    if let Some(last) = self.last.as_mut() {
                        last.delivered = false;
                    }
                    self.deliver(&mut out);
                }
            }
            RendererInput::DisplayListSendFailed => {
                self.sender = None;
                if let Some(last) = self.last.as_mut() {
                    last.delivered = false;
                }
            }
            RendererInput::Ping(n) => out.push(RendererOutput::Up(FromRenderer::Pong(n))),
            // Nothing after Exit: not even a layout the deadline would start.
            RendererInput::Shutdown => return vec![RendererOutput::Exit],
            // The deadline is checked below, after every input.
            RendererInput::Tick => {}
        }
        // On every input, not only on ticks: steady browser traffic must not keep the
        // renderer waiting past the deadline.
        if matches!(&self.doc, Doc::Awaiting { deadline, .. } if now >= *deadline) {
            self.attach_unanswered(&mut out);
        }
        out
    }

    fn load(
        &mut self,
        doc_id: u64,
        base_url: &str,
        bytes: &[u8],
        now: Instant,
        out: &mut Vec<RendererOutput>,
    ) {
        let parsed = Url::parse(base_url)
            .ok()
            .and_then(|base| ParsedDocument::parse(bytes, &base).ok());
        let Some(parsed) = parsed else {
            self.doc = Doc::Failed;
            self.answer(out);
            return;
        };
        // What fits the protocol is asked for; the rest renders as missing.
        let requested: Vec<Url> = parsed
            .subresource_requests()
            .iter()
            .filter(|url| url.as_str().len() <= MAX_URL_BYTES)
            .take(MAX_SUBRESOURCES_PER_DOCUMENT)
            .cloned()
            .collect();
        if requested.is_empty() {
            self.doc = Doc::Loaded(parsed.attach(SubresourceMap::new()));
            self.answer(out);
            return;
        }
        let urls = requested
            .iter()
            .map(|url| url.as_str().to_owned())
            .collect();
        self.doc = Doc::Awaiting {
            doc_id,
            parsed,
            requested,
            deadline: now + SUBRESOURCE_TIMEOUT,
        };
        out.push(RendererOutput::Up(FromRenderer::NeedSubresources {
            doc_id,
            urls,
        }));
    }

    fn on_subresources(
        &mut self,
        doc_id: u64,
        replies: Vec<(u32, Result<Vec<u8>, SubresourceRefusal>)>,
        out: &mut Vec<RendererOutput>,
    ) {
        let Doc::Awaiting {
            doc_id: awaited,
            requested,
            ..
        } = &self.doc
        else {
            return;
        };
        if *awaited != doc_id {
            return;
        }
        let mut map = SubresourceMap::new();
        for (index, result) in replies {
            let Some(url) = usize::try_from(index).ok().and_then(|i| requested.get(i)) else {
                continue;
            };
            match result {
                Ok(bytes) => map.insert_ok(url.clone(), bytes),
                Err(refusal) => map.insert_err(url.clone(), refusal_text(refusal).to_owned()),
            }
        }
        self.attach(map, out);
    }

    /// The subresource deadline passed: render with a warning per requested URL.
    fn attach_unanswered(&mut self, out: &mut Vec<RendererOutput>) {
        let Doc::Awaiting { requested, .. } = &self.doc else {
            return;
        };
        let mut map = SubresourceMap::new();
        for url in requested {
            map.insert_err(
                url.clone(),
                format!(
                    "the browser did not answer within {} s",
                    SUBRESOURCE_TIMEOUT.as_secs()
                ),
            );
        }
        self.attach(map, out);
    }

    fn attach(&mut self, map: SubresourceMap, out: &mut Vec<RendererOutput>) {
        let Doc::Awaiting { parsed, .. } = std::mem::replace(&mut self.doc, Doc::None) else {
            return;
        };
        self.doc = Doc::Loaded(parsed.attach(map));
        self.answer(out);
    }

    /// A `Redraw`: the current list again with the new `seq` if it was laid out for the
    /// current viewport, otherwise a new layout.
    fn redraw(&mut self, out: &mut Vec<RendererOutput>) {
        let current = self.viewport;
        let seq = self.seq;
        if matches!(self.doc, Doc::Loaded(_))
            && let Some(last) = self.last.as_mut().filter(|l| Some(l.viewport) == current)
        {
            last.seq = seq;
            last.delivered = false;
            let warnings = last.warnings.clone();
            self.deliver(out);
            out.push(RendererOutput::Up(FromRenderer::DocumentReady {
                seq,
                warnings,
            }));
            return;
        }
        self.answer(out);
    }

    /// Answers the current `seq` for the current viewport — unless the document is still
    /// loading, in which case the answer comes when it is.
    fn answer(&mut self, out: &mut Vec<RendererOutput>) {
        let seq = self.seq;
        let failed = |kind| RendererOutput::Up(FromRenderer::RenderFailed { seq, kind });
        let (doc, viewport) = match (&mut self.doc, self.viewport) {
            (Doc::None | Doc::Awaiting { .. }, _) | (Doc::Loaded(_), None) => return,
            (Doc::Failed, _) => {
                out.push(failed(RenderFailureKind::Parse));
                return;
            }
            (Doc::Loaded(doc), Some(viewport)) => (doc, viewport),
        };
        self.last = None;
        let size = (viewport.width(), viewport.height());
        let rendered = match doc.render(size, &mut self.fonts, StageDumps::None) {
            Ok(rendered) => rendered,
            Err(e) => {
                out.push(failed(failure_kind(&e)));
                return;
            }
        };
        let bytes = match cl_paint::wire::encode_display_list(&rendered.display_list) {
            Ok(bytes) if bytes.len() <= self.max_list_bytes => bytes,
            _ => {
                out.push(failed(RenderFailureKind::Encode));
                return;
            }
        };
        let warnings = bounded_warnings(&rendered.warnings);
        self.last = Some(LastList {
            seq,
            viewport,
            bytes,
            warnings: warnings.clone(),
            delivered: false,
        });
        self.deliver(out);
        out.push(RendererOutput::Up(FromRenderer::DocumentReady {
            seq,
            warnings,
        }));
    }

    /// Sends the last list if it has not reached a working sender yet.
    fn deliver(&mut self, out: &mut Vec<RendererOutput>) {
        if self.sender.is_none() {
            return;
        }
        if let Some(last) = self.last.as_mut().filter(|l| !l.delivered) {
            last.delivered = true;
            out.push(RendererOutput::SendDisplayList {
                seq: last.seq,
                bytes: last.bytes.clone(),
            });
        }
    }
}

/// The cause a refused subresource's warning names.
fn refusal_text(refusal: SubresourceRefusal) -> &'static str {
    match refusal {
        SubresourceRefusal::NotFile => "refused: not a local file: URL",
        SubresourceRefusal::OutsideDocumentDirectory => "refused: outside the document directory",
        SubresourceRefusal::NotRegularFile => "refused: not a regular file",
        SubresourceRefusal::TooLarge => "refused: the document's subresource budget is spent",
        SubresourceRefusal::TooMany => "refused: too many subresources",
        SubresourceRefusal::Io => "refused: could not be read",
    }
}

/// The stage a [`RenderError`] belongs to.
fn failure_kind(error: &RenderError) -> RenderFailureKind {
    match error {
        RenderError::Html(_) => RenderFailureKind::Parse,
        RenderError::Style(_) => RenderFailureKind::Style,
        RenderError::Layout(_) | RenderError::DocumentLost => RenderFailureKind::Layout,
    }
}

/// At most [`MAX_WARNINGS`] warnings, each cut to [`MAX_WARNING_BYTES`] at a char boundary.
fn bounded_warnings(warnings: &[String]) -> Vec<String> {
    warnings
        .iter()
        .take(MAX_WARNINGS)
        .map(|w| {
            let mut end = w.len().min(MAX_WARNING_BYTES);
            while !w.is_char_boundary(end) {
                end -= 1;
            }
            w.get(..end).unwrap_or_default().to_owned()
        })
        .collect()
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "a failed setup step should abort the test, loudly"
)]
mod tests {
    use std::fmt::Write as _;
    use std::time::{Duration, Instant};

    use cl_ipc::proto::renderer::{FromRenderer, RenderFailureKind, SubresourceRefusal};
    use cl_ipc::proto::{MAX_SUBRESOURCES_PER_DOCUMENT, MAX_WARNING_BYTES, MAX_WARNINGS, Viewport};
    use cl_paint::wire::{MAX_DISPLAY_LIST_BYTES, decode_display_list, encode_display_list};

    use super::{RendererInput, RendererOutput, RendererState, SUBRESOURCE_TIMEOUT};

    const BASE: &str = "file:///doc/page.html";
    const LINKED: &str = "<!doctype html><link rel=stylesheet href=a.css><p>hi</p>";
    const PLAIN: &str = "<!doctype html><p>hi</p>";

    fn vp(width: u32, height: u32) -> Viewport {
        Viewport::new(width, height).expect("viewport")
    }

    fn state() -> RendererState {
        RendererState::new(MAX_DISPLAY_LIST_BYTES).expect("fonts")
    }

    fn load(html: &str, viewport: Viewport, seq: u64) -> RendererInput {
        RendererInput::Load {
            doc_id: 1,
            base_url: BASE.to_owned(),
            viewport,
            seq,
            bytes: html.as_bytes().to_vec(),
        }
    }

    /// A state with a working display-list sender (generation 1).
    fn connected() -> (RendererState, Instant) {
        let t0 = Instant::now();
        let mut s = state();
        assert_eq!(
            s.on(RendererInput::DisplayListSender { generation: 1 }, t0),
            []
        );
        (s, t0)
    }

    /// The seqs of the lists sent and the `DocumentReady`s, in order: `('L', seq)` / `('R', seq)`.
    fn shape(outputs: &[RendererOutput]) -> Vec<(char, u64)> {
        outputs
            .iter()
            .map(|o| match o {
                RendererOutput::SendDisplayList { seq, .. } => ('L', *seq),
                RendererOutput::Up(FromRenderer::DocumentReady { seq, .. }) => ('R', *seq),
                RendererOutput::Up(FromRenderer::RenderFailed { seq, .. }) => ('F', *seq),
                RendererOutput::Up(FromRenderer::NeedSubresources { .. }) => ('N', 0),
                RendererOutput::Up(FromRenderer::Pong(n)) => ('P', *n),
                RendererOutput::Exit => ('X', 0),
            })
            .collect()
    }

    fn list_bytes(outputs: &[RendererOutput]) -> Vec<u8> {
        outputs
            .iter()
            .find_map(|o| match o {
                RendererOutput::SendDisplayList { bytes, .. } => Some(bytes.clone()),
                _ => None,
            })
            .expect("a display list was sent")
    }

    fn warnings(outputs: &[RendererOutput]) -> Vec<String> {
        outputs
            .iter()
            .find_map(|o| match o {
                RendererOutput::Up(FromRenderer::DocumentReady { warnings, .. }) => {
                    Some(warnings.clone())
                }
                _ => None,
            })
            .expect("DocumentReady")
    }

    fn replies(results: Vec<Result<Vec<u8>, SubresourceRefusal>>) -> RendererInput {
        RendererInput::Subresources {
            doc_id: 1,
            replies: results
                .into_iter()
                .enumerate()
                .map(|(i, r)| (u32::try_from(i).expect("small"), r))
                .collect(),
        }
    }

    #[test]
    fn a_document_without_subresources_renders_at_once() {
        let (mut s, t0) = connected();
        let out = s.on(load(PLAIN, vp(800, 600), 1), t0);
        assert_eq!(shape(&out), [('L', 1), ('R', 1)]);
        let dl = decode_display_list(&list_bytes(&out), MAX_DISPLAY_LIST_BYTES).expect("decode");
        assert_eq!(dl.bounds, cl_layout::Rect::from_px(0.0, 0.0, 800.0, 600.0));
        assert!(warnings(&out).is_empty());
    }

    #[test]
    fn load_requests_subresources_then_renders() {
        let (mut s, t0) = connected();
        assert_eq!(
            s.on(load(LINKED, vp(800, 600), 1), t0),
            [RendererOutput::Up(FromRenderer::NeedSubresources {
                doc_id: 1,
                urls: vec!["file:///doc/a.css".to_owned()]
            })]
        );
        let out = s.on(replies(vec![Ok(b"p { color: red }".to_vec())]), t0);
        assert_eq!(shape(&out), [('L', 1), ('R', 1)]);
        assert!(warnings(&out).is_empty());
    }

    #[test]
    fn a_refused_subresource_is_a_warning() {
        let (mut s, t0) = connected();
        let _ = s.on(load(LINKED, vp(800, 600), 1), t0);
        let out = s.on(
            replies(vec![Err(SubresourceRefusal::OutsideDocumentDirectory)]),
            t0,
        );
        assert_eq!(shape(&out), [('L', 1), ('R', 1)]);
        let warnings = warnings(&out);
        assert_eq!(warnings.len(), 1);
        let warning = warnings.first().expect("one");
        assert!(warning.starts_with("file:///doc/a.css: "), "{warning}");
        assert!(
            warning.contains("outside the document directory"),
            "{warning}"
        );
    }

    #[test]
    fn a_reply_for_another_document_is_ignored() {
        let (mut s, t0) = connected();
        let _ = s.on(load(LINKED, vp(800, 600), 1), t0);
        let other = RendererInput::Subresources {
            doc_id: 2,
            replies: vec![],
        };
        assert_eq!(s.on(other, t0), []);
    }

    #[test]
    fn shutdown_honored_while_awaiting_subresources() {
        let (mut s, t0) = connected();
        let _ = s.on(load(LINKED, vp(800, 600), 1), t0);
        assert_eq!(s.on(RendererInput::Shutdown, t0), [RendererOutput::Exit]);
    }

    #[test]
    fn shutdown_past_the_deadline_exits_without_rendering() {
        let (mut s, t0) = connected();
        let _ = s.on(load(LINKED, vp(800, 600), 1), t0);
        assert_eq!(
            s.on(RendererInput::Shutdown, t0 + SUBRESOURCE_TIMEOUT),
            [RendererOutput::Exit]
        );
    }

    #[test]
    fn subresource_timeout_renders_with_warnings() {
        let (mut s, t0) = connected();
        let _ = s.on(load(LINKED, vp(800, 600), 1), t0);
        let almost = t0 + SUBRESOURCE_TIMEOUT.saturating_sub(Duration::from_millis(1));
        assert_eq!(s.on(RendererInput::Tick, almost), []);
        let out = s.on(RendererInput::Tick, t0 + SUBRESOURCE_TIMEOUT);
        assert_eq!(shape(&out), [('L', 1), ('R', 1)]);
        assert_eq!(
            warnings(&out),
            ["file:///doc/a.css: the browser did not answer within 5 s"]
        );
        // A late answer changes nothing.
        assert_eq!(
            s.on(replies(vec![Ok(Vec::new())]), t0 + Duration::from_secs(6)),
            []
        );
    }

    #[test]
    fn resize_during_await_renders_latest_only() {
        let (mut s, t0) = connected();
        let _ = s.on(load(LINKED, vp(800, 600), 1), t0);
        for (viewport, seq) in [(vp(400, 300), 2), (vp(300, 200), 3)] {
            assert_eq!(s.on(RendererInput::Resize { viewport, seq }, t0), []);
        }
        let out = s.on(replies(vec![Ok(Vec::new())]), t0);
        assert_eq!(shape(&out), [('L', 3), ('R', 3)]);
        let dl = decode_display_list(&list_bytes(&out), MAX_DISPLAY_LIST_BYTES).expect("decode");
        assert_eq!(dl.bounds, cl_layout::Rect::from_px(0.0, 0.0, 300.0, 200.0));
    }

    #[test]
    fn redraw_during_load_sets_the_first_seq() {
        let (mut s, t0) = connected();
        let _ = s.on(load(LINKED, vp(800, 600), 1), t0);
        assert_eq!(s.on(RendererInput::Redraw { seq: 2 }, t0), []);
        let out = s.on(replies(vec![Ok(Vec::new())]), t0);
        assert_eq!(shape(&out), [('L', 2), ('R', 2)]);
    }

    #[test]
    fn a_redraw_resends_the_current_list_with_the_new_seq() {
        let (mut s, t0) = connected();
        let first = s.on(load(PLAIN, vp(800, 600), 1), t0);
        let again = s.on(RendererInput::Redraw { seq: 2 }, t0);
        assert_eq!(shape(&again), [('L', 2), ('R', 2)]);
        assert_eq!(list_bytes(&again), list_bytes(&first));
    }

    #[test]
    fn redraw_after_a_failed_render_renders_again() {
        // A document whose base URL does not parse never renders: every seq the browser asks
        // for is still answered, with RenderFailed for that seq.
        let (mut s, t0) = connected();
        let bad = RendererInput::Load {
            doc_id: 1,
            base_url: "not a url".to_owned(),
            viewport: vp(800, 600),
            seq: 1,
            bytes: PLAIN.as_bytes().to_vec(),
        };
        let failed = |seq| {
            RendererOutput::Up(FromRenderer::RenderFailed {
                seq,
                kind: RenderFailureKind::Parse,
            })
        };
        assert_eq!(s.on(bad, t0), [failed(1)]);
        assert_eq!(s.on(RendererInput::Redraw { seq: 2 }, t0), [failed(2)]);
        assert_eq!(
            s.on(
                RendererInput::Resize {
                    viewport: vp(400, 300),
                    seq: 3
                },
                t0
            ),
            [failed(3)]
        );
    }

    #[test]
    fn redraw_after_a_failed_resize_lays_out_for_the_latest_viewport() {
        // Narrow text wraps into more runs, so its list is larger; a byte cap between the two
        // sizes makes the narrow layout fail to encode, the wide one pass.
        let html = format!("<!doctype html><p>{}</p>", "word ".repeat(200));
        let size_at = |width: u32| {
            let base = cl_net::Url::parse(BASE).expect("base");
            let parsed = cl_renderer::ParsedDocument::parse(html.as_bytes(), &base).expect("parse");
            let mut doc = parsed.attach(cl_renderer::SubresourceMap::new());
            let mut fonts = cl_fonts::FontDb::bundled().expect("fonts");
            let rendered = doc
                .render((width, 600), &mut fonts, cl_renderer::StageDumps::None)
                .expect("render");
            encode_display_list(&rendered.display_list)
                .expect("encode")
                .len()
        };
        let (wide, narrow) = (size_at(800), size_at(60));
        assert!(narrow > wide, "{narrow} > {wide}");

        let t0 = Instant::now();
        let mut s = RendererState::new(wide).expect("fonts");
        let _ = s.on(RendererInput::DisplayListSender { generation: 1 }, t0);
        assert_eq!(
            shape(&s.on(load(&html, vp(800, 600), 1), t0)),
            [('L', 1), ('R', 1)]
        );
        let failed = |seq| {
            RendererOutput::Up(FromRenderer::RenderFailed {
                seq,
                kind: RenderFailureKind::Encode,
            })
        };
        let resize = |viewport, seq| RendererInput::Resize { viewport, seq };
        assert_eq!(s.on(resize(vp(60, 600), 2), t0), [failed(2)]);
        // The 800-wide list must not come back for the 60-wide surface.
        assert_eq!(s.on(RendererInput::Redraw { seq: 3 }, t0), [failed(3)]);
        assert_eq!(
            shape(&s.on(resize(vp(800, 600), 4), t0)),
            [('L', 4), ('R', 4)]
        );
    }

    #[test]
    fn new_display_list_sender_triggers_resend() {
        let (mut s, t0) = connected();
        let first = s.on(load(PLAIN, vp(800, 600), 1), t0);
        // The gpu is gone: the send failed. Nothing to do until a newer gpu's sender arrives.
        assert_eq!(s.on(RendererInput::DisplayListSendFailed, t0), []);
        assert_eq!(
            s.on(RendererInput::DisplayListSender { generation: 1 }, t0),
            []
        );
        let resent = s.on(RendererInput::DisplayListSender { generation: 2 }, t0);
        assert_eq!(shape(&resent), [('L', 1)]);
        assert_eq!(list_bytes(&resent), list_bytes(&first));
        // A generation that is not newer is not a new gpu.
        assert_eq!(
            s.on(RendererInput::DisplayListSender { generation: 2 }, t0),
            []
        );
    }

    #[test]
    fn a_list_sent_to_a_dying_gpu_reaches_the_next_one() {
        // The browser kills gpu 1 and asks for a Redraw before gpu 2's sender arrives: the
        // send to gpu 1 can still succeed and the list die with it. A new generation has never
        // seen the list, so it gets it.
        let (mut s, t0) = connected();
        let _ = s.on(load(PLAIN, vp(800, 600), 1), t0);
        assert_eq!(
            shape(&s.on(RendererInput::Redraw { seq: 2 }, t0)),
            [('L', 2), ('R', 2)]
        );
        assert_eq!(
            shape(&s.on(RendererInput::DisplayListSender { generation: 2 }, t0)),
            [('L', 2)]
        );
    }

    #[test]
    fn the_subresource_deadline_holds_under_steady_traffic() {
        // Pings arriving faster than the shell's tick must not keep the renderer waiting.
        let (mut s, t0) = connected();
        let _ = s.on(load(LINKED, vp(800, 600), 1), t0);
        let out = s.on(RendererInput::Ping(9), t0 + SUBRESOURCE_TIMEOUT);
        assert_eq!(shape(&out), [('P', 9), ('L', 1), ('R', 1)]);
    }

    #[test]
    fn warnings_are_cut_at_a_char_boundary_and_in_number() {
        // 'é' is two bytes: one straddles byte MAX_WARNING_BYTES.
        let straddling = format!("{}é", "x".repeat(MAX_WARNING_BYTES - 1));
        let cut = super::bounded_warnings(&[straddling]);
        assert_eq!(cut, ["x".repeat(MAX_WARNING_BYTES - 1)]);
        let many = vec!["w".to_owned(); MAX_WARNINGS + 5];
        assert_eq!(super::bounded_warnings(&many).len(), MAX_WARNINGS);
    }

    #[test]
    fn a_list_made_before_any_sender_waits_for_the_first_one() {
        let t0 = Instant::now();
        let mut s = state();
        assert_eq!(shape(&s.on(load(PLAIN, vp(800, 600), 1), t0)), [('R', 1)]);
        assert_eq!(
            shape(&s.on(RendererInput::DisplayListSender { generation: 1 }, t0)),
            [('L', 1)]
        );
    }

    #[test]
    fn requests_and_warnings_stay_within_the_protocol_limits() {
        // Past 64 links, and a link longer than a URL may be on the wire: neither is
        // requested (the browser would kill the renderer), both render as missing; every
        // warning is cut to MAX_WARNING_BYTES.
        let mut html = String::from("<!doctype html>");
        for i in 0..MAX_SUBRESOURCES_PER_DOCUMENT + 2 {
            write!(html, "<link rel=stylesheet href=s{i}.css>").expect("write to a String");
        }
        let long = "x".repeat(9 * 1024);
        write!(html, "<link rel=stylesheet href={long}.css><p>hi</p>").expect("write to a String");
        let (mut s, t0) = connected();
        let out = s.on(load(&html, vp(800, 600), 1), t0);
        let [RendererOutput::Up(FromRenderer::NeedSubresources { urls, .. })] = out.as_slice()
        else {
            unreachable!("expected one NeedSubresources, got {out:?}")
        };
        let expected: Vec<String> = (0..MAX_SUBRESOURCES_PER_DOCUMENT)
            .map(|i| format!("file:///doc/s{i}.css"))
            .collect();
        assert_eq!(urls, &expected, "the first 64, in document order");
        let answers = vec![Ok(Vec::new()); MAX_SUBRESOURCES_PER_DOCUMENT];
        let out = s.on(replies(answers), t0);
        let warnings = warnings(&out);
        assert_eq!(warnings.len(), 3, "{warnings:?}");
        assert!(warnings.iter().all(|w| w.len() <= MAX_WARNING_BYTES));
    }

    #[test]
    fn ping_is_answered() {
        let (mut s, t0) = connected();
        assert_eq!(
            s.on(RendererInput::Ping(7), t0),
            [RendererOutput::Up(FromRenderer::Pong(7))]
        );
    }
}
