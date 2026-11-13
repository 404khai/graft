//! `{{ variable }}` templates (env files are repository contents): parse and
//! render arbitrary text; a successful render has no placeholders left
//! unless they were escaped.
#![no_main]

use std::collections::BTreeMap;

use graft_blueprint::template::{Origin, Template};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    let origin = Origin {
        file: "f".to_owned(),
        line: 1,
        column: 1,
    };
    let Ok(t) = Template::parse(text, &origin) else {
        return;
    };
    let vars: BTreeMap<String, String> = t
        .variables()
        .map(|(name, _, _)| (name.to_owned(), "x".to_owned()))
        .collect();
    let rendered = t.render(&vars).expect("every variable is set");
    if !text.contains("{{{{") {
        assert!(!rendered.contains("{{"), "{text:?} rendered as {rendered:?}");
    }
    let _ = t.render(&BTreeMap::new());
});
