//! A session playing a show with scenes: inputs given by hand, paused or
//! playing, and a scrub replaying them.

use cuelight_core::{Property, Value};
use cuelight_editor_core::session::{Instant, Session};

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
