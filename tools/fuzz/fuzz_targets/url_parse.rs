#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Untrusted input (an href, an attacker-controlled URL string) must never panic the WHATWG
    // URL parser (docs/CODING_STANDARDS.md §2). Exercise both the no-base and with-base entry
    // points: the with-base path resolves the fuzzed bytes as a relative reference against a
    // fixed absolute base, the same shape as resolving an `href`/`src`/`@import` against a
    // document's URL.
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = cl_net::Url::parse(s);
        if let Ok(base) = cl_net::Url::parse("http://example.com/a/b") {
            let _ = cl_net::Url::parse_with_base(s, &base);
        }
    }
});
