//! Radar's and the scheduler's tree-sitter analysis, on arbitrary source in
//! every supported language (files of the repository are untrusted).
#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: (u8, &[u8], &[u8])| {
    let (lang, old, new) = input;
    let ext = ["rs", "ts", "tsx", "py", "go"][usize::from(lang) % 5];
    let path = format!("f.{ext}");
    let path = Path::new(&path);
    if let Some(changed) = graft_radar::symbols::changed_symbols(path, Some(old), Some(new)) {
        // A file compared with itself changed nothing.
        if old == new {
            assert!(changed.is_empty());
        }
    }
    if let Some(l) = graft_radar::symbols::Lang::of(path) {
        let _ = graft_sched::analyze::analyze(l, new);
    }
});
