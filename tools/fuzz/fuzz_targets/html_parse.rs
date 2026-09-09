#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Drives the full hostile-input path: encoding sniff -> html5ever tokenizer/tree builder ->
    // arena DOM -> html5lib serializer. Any panic here is a security bug (docs/CODING_STANDARDS.md
    // §2) — `parse_document` is documented as effectively total over its input, and a tree the
    // parser can build but the serializer cannot print is a real bug too, so success is chased
    // with a serialize rather than immediately discarded.
    //
    // `expect` on a compile-time-constant, statically-valid URL is acceptable here: this is a
    // fuzz harness, not library code, and a harness that cannot even construct its own fixture
    // must fail loudly rather than silently skip every input.
    let base = cl_net::Url::parse("file:///fuzz.html").expect("static url");
    let _ = cl_html::parse_document(data, &base, None)
        .map(|o| cl_dom::serialize::html5lib_tree(&o.document));
});
