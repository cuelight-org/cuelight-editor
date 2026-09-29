//! The log: what the audit, the load, the trace and the hand put in it,
//! one line each, in columns.

// Test code throughout, so clippy lets it panic as tests do.
#![cfg(test)]

use cuelight_core::{FindingKind, Value};
use cuelight_editor_core::log::{self, Kind, Line, Log};
use cuelight_editor_core::session::{Instant, Session};
use cuelight_loader::Step;

const SHOW: &str = r##"{ "name": "logged", "size": [64, 32],
  "variables": { "score": 0 },
  "fonts": { "unused": { "file": "nope" } },
  "layers": [
    { "name": "box", "type": "shape", "shape": { "rect": [0, 0, 32, 32] }, "fill": "#fff",
      "timelines": [{ "name": "hop", "trigger": "go", "on_end": "landed",
        "tracks": [{ "property": "x", "keys": [{ "t": 0, "v": 0 }, { "t": 0.5, "v": 8 }] }] }] } ] }"##;

#[test]
fn a_line_says_its_kind_its_instant_and_its_text_in_columns() {
    let line = log::fired(1.5, "go");
    assert_eq!(line.render(), "input            1.50  fired \"go\"");
    let audit = Line {
        kind: Kind::Audit(FindingKind::Unused),
        at: None,
        text: "fonts.unused: font style \"unused\" is declared and no layer uses it".to_owned(),
    };
    assert_eq!(
        audit.render(),
        "audit unused           fonts.unused: font style \"unused\" is declared and no layer uses it"
    );
    assert_eq!(
        log::set(0.0, "score", &Value::Number(12.0)).text,
        "set \"score\" to 12"
    );
    assert!(log::driver(2.0, &Step::Wait { wait: 1.0 }).is_none());
    assert_eq!(
        log::driver(
            2.0,
            &Step::Trigger {
                trigger: "go".into()
            }
        )
        .map(|l| l.render()),
        Some("driver           2.00  fired \"go\"".to_owned())
    );
}

#[test]
fn the_audit_of_a_document_lands_in_the_log_with_its_paths() {
    let lines = log::audit(SHOW, &["show.json".to_owned()], None);
    let rendered: Vec<String> = lines.iter().map(Line::render).collect();
    assert!(
        rendered
            .iter()
            .any(|l| l.starts_with("audit unused") && l.contains("fonts.unused: ")),
        "{rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|l| l.starts_with("audit missing")
                && l.contains("fonts.unused: names font \"nope\"")),
        "{rendered:?}"
    );
    // The variable nothing reads.
    assert!(
        rendered.iter().any(|l| l.contains("variables.score")),
        "{rendered:?}"
    );
}

#[test]
fn the_log_keeps_at_most_its_cap_and_clears_by_kind() {
    let mut log = Log::default();
    log.extend(log::load(&["skipped x".to_owned()]));
    log.push(log::fired(0.0, "go"));
    for i in 0..log::CAP + 10 {
        log.push(log::fired(i as f64, "tick"));
    }
    assert_eq!(log.len(), log::CAP);
    // The oldest went first: the load line and the first input are gone.
    assert!(log.lines().all(|l| l.kind == Kind::Input));
    let mut log = Log::default();
    log.extend(log::load(&["skipped x".to_owned()]));
    log.push(Line {
        kind: Kind::Audit(FindingKind::Unwise),
        at: None,
        text: "x".into(),
    });
    log.push(log::fired(0.0, "go"));
    log.clear_played();
    assert_eq!(log.len(), 2);
    log.clear_audit();
    assert_eq!(log.len(), 1);
    assert_eq!(log.lines().next().map(|l| l.kind), Some(Kind::Load));
}

#[test]
fn a_session_logs_what_the_hand_did_and_what_the_engine_traced() {
    let mut engine = cuelight::Engine::new();
    engine.load_show(SHOW).unwrap();
    let mut session = Session::new(engine, None);
    session.audit(SHOW);
    let audits = session.log.len();
    assert!(audits >= 2, "{}", audits);
    let start = Instant::now();
    session.toggle_pause(start);
    session.fire("go");
    session.set("score", Value::Number(3.0));
    session.tick(start + std::time::Duration::from_millis(700));
    let rendered: Vec<String> = session.log.lines().map(Line::render).collect();
    assert!(
        rendered
            .iter()
            .any(|l| l.starts_with("input") && l.contains("fired \"go\"")),
        "{rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|l| l.starts_with("input") && l.contains("set \"score\" to 3")),
        "{rendered:?}"
    );
    // The trace says the timeline started and the on_end fired.
    assert!(
        rendered
            .iter()
            .any(|l| l.starts_with("trace") && l.contains("started")),
        "{rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|l| l.starts_with("trace") && l.contains("\"landed\"")),
        "{rendered:?}"
    );
    // A restart keeps what is said of the document and drops the rest.
    session.restart(Instant::now());
    assert_eq!(session.log.len(), audits);
    // An audit run again replaces the last one rather than adding to it.
    session.audit(SHOW);
    assert_eq!(session.log.len(), audits);
}

#[test]
fn the_audit_of_one_place_is_found_by_its_path() {
    let mut log = Log::default();
    log.extend(log::audit(
        SHOW,
        &["show.json".to_owned(), "assets/sounds/spare.wav".to_owned()],
        None,
    ));
    log.push(log::fired(
        1.0,
        "assets/sounds/spare.wav: not an audit line",
    ));
    let about: Vec<&str> = log
        .audit_of("assets/sounds/spare.wav")
        .map(|l| l.text.as_str())
        .collect();
    assert_eq!(
        about,
        ["assets/sounds/spare.wav: nothing in the show names this file"]
    );
    assert_eq!(log.audit_of("assets/sounds/spare").count(), 0);
}
