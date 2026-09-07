#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Both directions share one decoder; any panic here is a security bug (docs/CODING_STANDARDS.md §2).
    let _ = cl_ipc::codec::decode::<cl_ipc::message::ToBrowser>(data);
    let _ = cl_ipc::codec::decode::<cl_ipc::message::ToChild>(data);
});
