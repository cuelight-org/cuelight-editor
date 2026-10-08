//! A session playing a show with scenes: inputs given by hand, paused or
//! playing, and a scrub replaying them.

// Test code throughout, so clippy lets it panic as tests do.
#![cfg(test)]

use cuelight_core::{Property, Value};
use cuelight_editor_core::session::{Instant, Session, What};

/// Two scenes: the first entered at load, the second by `next`, where a
/// bar slides from 5 to 15 over a second from the moment it is entered.
const SHOW: &str = r##"{ "name": "scenes", "size": [64, 32],
  "scenes": [
    { "name": "intro", "layers": [
      { "name": "title", "type": "shape", "shape": { "rect": [0, 0, 8, 8] }, "fill": "#fff" } ] },
    { "name": "second", "trigger": "next", "layers": [
      { "name": "bar", "type": "shape", "shape": { "rect": [0, 0, 8, 8] }, "fill": "#fff",
        "timelines": [{ "name": "slide", "autoplay": true, "hold": true,
          "tracks": [{ "property": "x", "keys": [{ "t": 0, "v": 5 }, { "t": 1, "v": 15 }] }] }] } ] } ] }"##;

fn session() -> Session {
    let mut engine = cuelight::Engine::new();
    engine.load_show(SHOW).unwrap();
    Session::new(engine, None)
}

fn x_of(session: &Session, layer: &str) -> Option<f64> {
    let engine = session.engine.lock().unwrap();
    engine
        .values()
        .unwrap()
        .into_iter()
        .find(|v| v.name == layer && v.property == Property::X)
        .and_then(|v| match v.value {
            Value::Number(n) => Some(n),
            _ => None,
        })
}

#[test]
fn a_trigger_fired_while_paused_enters_its_scene_and_stays_paused() {
    let mut session = session();
    let now = Instant::now();
    session.seek(1.0, now);
    assert_eq!(session.active_scene().as_deref(), Some("intro"));
    let revision = session.revision;

    session.fire("next");
    assert!(session.paused, "the show stays paused");
    assert_eq!(session.time, 1.0, "at the paused instant");
    assert_eq!(session.engine.lock().unwrap().time(), 1.0);
    assert!(session.revision > revision, "the stage is told to redraw");
    assert_eq!(session.active_scene().as_deref(), Some("second"));
    assert_eq!(x_of(&session, "bar"), Some(5.0), "the slide starts there");

    // A frame while paused moves nothing.
    session.tick(now + std::time::Duration::from_secs(3));
    assert_eq!(session.time, 1.0);
    assert_eq!(x_of(&session, "bar"), Some(5.0));

    // The input was recorded: a scrub before it leaves the scene, one
    // after it replays it.
    session.seek(0.5, now);
    assert_eq!(session.active_scene().as_deref(), Some("intro"));
    session.seek(1.5, now);
    assert_eq!(session.active_scene().as_deref(), Some("second"));
    assert_eq!(x_of(&session, "bar"), Some(10.0));
}

#[test]
fn a_key_or_a_set_while_paused_leaves_the_show_paused() {
    let mut session = session();
    session.set("unknown", Value::Bool(true));
    assert!(session.paused);
    assert_eq!(session.key("x"), None, "the show maps no keys");
    assert!(session.paused);
    assert_eq!(session.time, 0.0);
}

#[test]
fn a_scene_entered_by_its_heading_counts_its_own_clock() {
    let mut session = session();
    let now = Instant::now();
    session.toggle_pause(now);
    session.tick(now);
    session.tick(now + std::time::Duration::from_millis(2500));
    assert!(!session.paused);
    assert_eq!(session.entered, 0.0, "the first scene was entered at load");

    // The second scene is entered by its trigger, paused where it was.
    session.enter_scene(1, now);
    assert!(session.paused);
    assert_eq!(session.active_scene().as_deref(), Some("second"));
    assert_eq!(session.entered, 2.5);
    assert_eq!(session.scene_length(), 1.0, "its slide is a second long");
    assert!(matches!(
        session.happened.back().map(|h| &h.what),
        Some(What::Fired(t)) if t == "next"
    ));

    // Where it was entered from, and by what.
    let last = session.entries.last().cloned().unwrap();
    assert_eq!(
        (last.at, last.scene.as_str(), last.from.as_deref()),
        (2.5, "second", Some("intro"))
    );
    assert!(last.by.contains("\"next\""), "{}", last.by);

    // Within the scene: a seek to its entry plus a time.
    session.seek(session.entered + 0.5, now);
    assert_eq!(session.time, 3.0);
    assert_eq!(session.entered, 2.5, "a scrub replays the entry");
    assert_eq!(session.entries.last(), Some(&last), "and keeps it");
    session.seek(1.0, now);
    assert!(
        session.entries.iter().all(|e| e.scene == "intro"),
        "a seek back takes later entries out"
    );
    session.seek(3.0, now);
    assert_eq!(x_of(&session, "bar"), Some(10.0));

    // The first scene has no trigger: entering it restarts the show.
    assert!(session.enter_scene(0, now));
    assert!(session.paused);
    assert_eq!(session.time, 0.0);
    assert_eq!(session.entered, 0.0);
    assert_eq!(session.active_scene().as_deref(), Some("intro"));
    assert_eq!(session.scene_length(), 0.0, "it has no timelines");
}
