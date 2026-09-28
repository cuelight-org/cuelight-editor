//! An edited document reloads into the session and lands back at the
//! playhead, inputs replayed.

use std::path::{Path, PathBuf};
use std::time::Instant;

use cuelight_core::Property;
use cuelight_editor_core::document::Pointer;
use cuelight_editor_core::opened::Opened;
use cuelight_editor_core::session::Session;
use serde_json::json;

fn fixture() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/mini"))
}

/// The examples checkout, beside this repository or where
/// `CUELIGHT_EXAMPLES` says.
fn examples() -> Option<PathBuf> {
    let dir = std::env::var_os("CUELIGHT_EXAMPLES")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../cuelight-examples")
        });
    dir.join("deck/show.json").is_file().then_some(dir)
}

fn value_of(session: &Session, layer: &str, property: Property) -> Option<f64> {
    let engine = session.engine.lock().unwrap();
    engine
        .values()
        .unwrap()
        .into_iter()
        .find(|v| v.name == layer && v.property == property)
        .and_then(|v| match v.value {
            cuelight_core::Value::Number(n) => Some(n),
            _ => None,
        })
}

#[test]
fn an_edit_reloads_at_the_playhead_with_the_inputs_replayed() {
    let opened = Opened::from_path(fixture()).unwrap();
    let mut document = opened.document;
    let mut session = Session::new(opened.engine, opened.driver);
    let now = Instant::now();
    // Fire the hop by hand at 0.3 s and stand at 0.35 s, mid-hop.
    session.seek(0.3, now);
    session.fire("go");
    session.seek(0.35, now);
    let y_before = value_of(&session, "dot", Property::Y).unwrap();
    assert!(y_before < 0.0, "mid-hop: {y_before}");
    assert_eq!(value_of(&session, "group", Property::X), Some(32.0));

    // Move the group, reload, and the show is where it was, moved.
    document
        .set(&Pointer::parse("/layers/1/x").unwrap(), json!(40))
        .unwrap();
    let findings = session.reload(&document.text(), now).unwrap();
    assert!(findings.is_empty(), "{findings:?}");
    assert_eq!(session.time, 0.35);
    assert!(session.paused, "as it was");
    assert_eq!(value_of(&session, "group", Property::X), Some(40.0));
    assert_eq!(
        value_of(&session, "dot", Property::Y),
        Some(y_before),
        "the hop still runs"
    );

    // A document that drops a layer says so, and the rest reloads.
    document
        .set(&Pointer::parse("/layers/0/shape").unwrap(), json!("far"))
        .unwrap();
    let findings = session.reload(&document.text(), now).unwrap();
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].path.starts_with("layers[0]"), "{findings:?}");
    assert_eq!(value_of(&session, "group", Property::X), Some(40.0));

    // Text that is no document at all is refused, and the show stays.
    assert!(session.reload("{", now).is_err());
    assert_eq!(value_of(&session, "group", Property::X), Some(40.0));
}

#[test]
fn a_reload_of_the_deck_returns_to_the_playhead_within_a_frame() {
    let Some(examples) = examples() else {
        eprintln!("no cuelight-examples checkout: the deck budget is not measured");
        return;
    };
    let opened = Opened::from_path(&examples.join("deck")).unwrap();
    let document = opened.document;
    let mut session = Session::new(opened.engine, opened.driver);
    let now = Instant::now();
    session.seek(20.0, now);
    let text = document.text();
    let started = Instant::now();
    session.reload(&text, now).unwrap();
    let took = started.elapsed();
    eprintln!(
        "deck: reload and return to 20 s took {:.2} ms",
        took.as_secs_f64() * 1e3
    );
    assert_eq!(session.time, 20.0);
    if !cfg!(debug_assertions) {
        assert!(took.as_millis() < 16, "{took:?} is more than a frame");
    }
}
