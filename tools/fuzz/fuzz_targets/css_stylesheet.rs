#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Untrusted CSS (an author stylesheet, a `<style>` element, an `@import`ed resource) must
    // never panic the cssparser/stylo parsing path (docs/CODING_STANDARDS.md §2). A fresh
    // engine per input keeps each run independent — construction is cheap next to parsing an
    // arbitrary stylesheet, so there is no need to share one across iterations via
    // `thread_local`.
    if let Ok(css) = std::str::from_utf8(data) {
        let Ok(mut engine) = cl_style::StyleEngine::new((800.0, 600.0), 1.0) else {
            return;
        };
        // The base URL is a compile-time constant known to parse, so a failure here can only
        // mean the fuzzed CSS itself was rejected (`StyleError::Url` is unreachable) — either
        // way this is `Result` we deliberately discard, not one we `expect` on.
        let _ = engine.add_author_sheet(css, "file:///fuzz.css");
    }
});
