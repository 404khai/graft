//! The example `graft.toml` files in `examples/` parse, so they cannot
//! drift from the format they document.

use std::path::Path;

#[test]
fn every_example_graft_toml_parses() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut checked = 0;
    for entry in std::fs::read_dir(&examples).unwrap() {
        let file = entry.unwrap().path().join("graft.toml");
        if !file.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap();
        let name = file.display().to_string();
        graft_blueprint::parse_layers(&text, &name).unwrap();
        graft_blueprint::parse_sandbox(&text, &name).unwrap();
        graft_blueprint::parse_radar(&text, &name).unwrap();
        graft_blueprint::parse_integrate(&text, &name).unwrap();
        graft_blueprint::parse_blueprints(&text, &name).unwrap();
        checked += 1;
    }
    assert!(checked > 0, "no examples/*/graft.toml found");
}
