//! Every `graft.toml` parser, on arbitrary text: repository contents are
//! untrusted (AGENTS.md §11), so they must fail with an error, never panic.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    let _ = graft_blueprint::parse_layers(text, "graft.toml");
    let _ = graft_blueprint::parse_sandbox(text, "graft.toml");
    let _ = graft_blueprint::parse_radar(text, "graft.toml");
    let _ = graft_blueprint::parse_integrate(text, "graft.toml");
    if let Ok(bps) = graft_blueprint::parse_blueprints(text, "graft.toml") {
        // Errors must point inside the file.
        for bp in bps.values() {
            for (_, t) in &bp.env {
                for (_, line, column) in t.variables() {
                    assert!(line >= 1 && column >= 1);
                }
            }
        }
    }
});
