use super::*;
use crate::stage::{Grip, Held};
use cuelight_core::Value;
use cuelight_editor_core::placement::ClipHandle;
use cuelight_editor_core::session::{Step, What};
use iced_test::simulator;

#[test]
fn the_command_line_is_read() {
    use clap::Parser;
    let options = Options::try_parse_from([
        "cuelight-editor",
        "deck",
        "--zoom",
        "2",
        "--pick",
        "10, 20",
        "--asset",
        "robot",
        "--silent",
        "--screenshot",
        "out.png",
    ])
    .unwrap();
    assert_eq!(
        options,
        Options {
            show: Some("deck".into()),
            zoom: Some(2.0),
            pick: Some([10.0, 20.0]),
            asset: Some("robot".to_owned()),
            solo: false,
            trigger: Vec::new(),
            silent: true,
            screenshot: Some("out.png".into()),
        }
    );
    assert!(Options::try_parse_from(["cuelight-editor", "--pick", "10"]).is_err());
    assert!(Options::try_parse_from(["cuelight-editor", "--zoom", "big"]).is_err());
}

#[test]
fn starts_with_an_open_button_and_a_hint() {
    let (app, _) = App::new();
    let mut ui = simulator(app.view());
    assert!(ui.find("Open file...").is_ok());
    assert!(ui.find("Open a show folder, a packed show (.cuelight) or a show.json, or drop one on this window.").is_ok());
}

#[test]
fn shows_what_it_opened() {
    let (mut app, _) = App::new();
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../cuelight-editor-core/tests/fixtures/mini"
    );
    let _ = app.update(Message::Dropped(dir.into()));
    assert!(app.status.starts_with("opened "), "{}", app.status);
    let mut ui = simulator(app.view());
    assert!(ui.find("show, format 1").is_ok());
    assert_eq!(app.field_text("size"), Some("64, 32"));
    assert!(ui.find("JSON").is_ok(), "the show's JSON is shown");
    assert!(ui.find("Play").is_ok(), "an opened show is paused at 0");
}

#[test]
fn the_show_is_picked_and_set_like_a_layer() {
    let (mut app, _) = App::new();
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../cuelight-editor-core/tests/fixtures/mini"
    );
    let _ = app.update(Message::Dropped(dir.into()));
    let path = app.rows.iter().find_map(|row| match row {
        cuelight_editor_core::tree::Row::Layer { path, .. } => Some(path.clone()),
        _ => None,
    });
    let _ = app.update(Message::Choose(path.unwrap()));
    assert_eq!(
        app.field_text("background"),
        None,
        "a layer has no background"
    );
    // The tree's SHOW heading picks the show.
    let mut ui = simulator(app.view());
    let _ = ui.click("SHOW");
    for message in ui.into_messages() {
        let _ = app.update(message);
    }
    assert!(app.selection.is_empty());
    let output = |app: &App| app.document.as_ref().unwrap().value()["output"].clone();

    let _ = app.update(Message::PutField("scaling", "pixel_perfect".into()));
    assert_eq!(output(&app)["scaling"], "pixel_perfect");
    let engine = lock(&app.session.as_ref().unwrap().engine);
    assert_eq!(engine.scaling(), cuelight_core::Scaling::PixelPerfect);
    drop(engine);

    // A default is not written down: the key goes.
    let _ = app.update(Message::PutField("scaling", "smooth".into()));
    assert!(output(&app).get("scaling").is_none(), "{}", output(&app));
    assert!(
        app.layer_fields
            .iter()
            .any(|f| f.field.label == "scaling" && !f.written)
    );

    // A colour field unfolds the same channel sliders as a property:
    // dragging types, letting go writes.
    let _ = app.update(Message::UnfoldField(Some("background")));
    {
        let mut ui = simulator(app.view());
        assert!(
            ui.find("R").is_ok() && ui.find("A").is_ok(),
            "the channels show"
        );
    }
    let _ = app.update(Message::TypeField("background", "#102030FF".into()));
    let _ = app.update(Message::ApplyField("background"));
    assert_eq!(
        app.document.as_ref().unwrap().value()["background"],
        "#102030FF"
    );

    // Typed, applied on leaving: the name.
    let _ = app.update(Message::TypeField("name", "renamed".into()));
    let _ = app.update(Message::Commit);
    assert_eq!(app.document.as_ref().unwrap().value()["name"], "renamed");
    // A show cannot do without its name: the reset is refused, and undone.
    let _ = app.update(Message::ResetField("name"));
    assert_eq!(app.document.as_ref().unwrap().value()["name"], "renamed");
}

#[test]
fn an_edited_show_is_saved_with_ctrl_s() {
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../cuelight-editor-core/tests/fixtures/mini"
    );
    let dir = tempfile::tempdir().unwrap();
    for name in ["show.json", "test-driver.json", "assets/dot.png"] {
        let to = dir.path().join(name);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(std::path::Path::new(fixture).join(name), to).unwrap();
    }
    let (mut app, _) = App::new();
    let _ = app.update(Message::Dropped(dir.path().into()));
    {
        let mut ui = simulator(app.view());
        assert!(ui.find("Save").is_ok());
    }
    assert_eq!(app.title(), "mini - cuelight editor");

    let size = cuelight_editor_core::document::Pointer::parse("/size/0").unwrap();
    app.document
        .as_mut()
        .unwrap()
        .set(&size, serde_json::json!(65))
        .unwrap();
    assert_eq!(app.title(), "mini* - cuelight editor");
    let _ = app.update(Message::KeyPressed(
        keyboard::Key::Character("s".into()),
        keyboard::Modifiers::CTRL,
    ));
    assert!(app.status.starts_with("saved "), "{}", app.status);
    assert_eq!(app.title(), "mini - cuelight editor");
    let saved = std::fs::read_to_string(dir.path().join("show.json")).unwrap();
    assert_eq!(saved, app.document.as_ref().unwrap().text());
    assert!(saved.contains("65"));
}

/// The mini fixture copied into a fresh folder, and the app opened on it.
fn open_copy() -> (App, tempfile::TempDir) {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../cuelight-editor-core/tests/fixtures/mini");
    let dir = tempfile::tempdir().unwrap();
    for name in ["show.json", "test-driver.json", "assets/dot.png"] {
        let to = dir.path().join(name);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(fixture.join(name), to).unwrap();
    }
    let (mut app, _) = App::new();
    let _ = app.update(Message::Dropped(dir.path().into()));
    (app, dir)
}

/// Write `show.json` from outside, renaming the show from `from` to
/// `name`, and say so.
fn rename_on_disk(app: &mut App, dir: &std::path::Path, from: &str, name: &str) {
    let show = dir.join("show.json");
    let text = std::fs::read_to_string(&show).unwrap().replace(
        &format!("\"name\": \"{from}\""),
        &format!("\"name\": \"{name}\""),
    );
    std::fs::write(&show, text).unwrap();
    let _ = app.update(Message::DiskChanged(vec![show]));
}

#[test]
fn a_show_changed_on_disk_reloads_at_the_playhead() {
    let (mut app, dir) = open_copy();
    let _ = app.update(Message::Seek(0.4));
    rename_on_disk(&mut app, dir.path(), "mini", "outside");
    assert_eq!(app.status, "reloaded show.json from disk");
    assert_eq!(app.summary.name, "outside");
    assert!((app.session.as_ref().unwrap().time - 0.4).abs() < 1e-9);
    assert!(
        app.document
            .as_ref()
            .unwrap()
            .text()
            .contains("\"outside\"")
    );

    // A new asset opens the show again, and still at the playhead.
    std::fs::copy(
        dir.path().join("assets/dot.png"),
        dir.path().join("assets/dot2.png"),
    )
    .unwrap();
    let _ = app.update(Message::DiskChanged(vec![
        dir.path().join("assets/dot2.png"),
    ]));
    assert_eq!(app.status, "reloaded assets/dot2.png from disk");
    assert!((app.session.as_ref().unwrap().time - 0.4).abs() < 1e-9);
}

#[test]
fn the_editors_own_save_reloads_nothing() {
    let (mut app, dir) = open_copy();
    let size = cuelight_editor_core::document::Pointer::parse("/size/0").unwrap();
    app.document
        .as_mut()
        .unwrap()
        .set(&size, serde_json::json!(65))
        .unwrap();
    let _ = app.update(Message::Save);
    let saved = app.status.clone();
    assert!(saved.starts_with("saved "), "{saved}");
    let _ = app.update(Message::DiskChanged(vec![dir.path().join("show.json")]));
    assert_eq!(app.status, saved);
    assert!(
        app.document.as_ref().unwrap().can_undo(),
        "the history stays"
    );
}

#[test]
fn a_change_on_disk_asks_before_dropping_unsaved_edits() {
    let (mut app, dir) = open_copy();
    let size = cuelight_editor_core::document::Pointer::parse("/size/0").unwrap();
    app.document
        .as_mut()
        .unwrap()
        .set(&size, serde_json::json!(65))
        .unwrap();
    rename_on_disk(&mut app, dir.path(), "mini", "outside");
    {
        let mut ui = simulator(app.view());
        assert!(ui.find("Load from disk").is_ok());
    }
    let _ = app.update(Message::KeepEdits);
    assert!(app.outside.is_none());
    assert!(
        app.document.as_ref().unwrap().is_dirty(),
        "the edit is kept"
    );
    assert_eq!(app.summary.name, "mini");

    rename_on_disk(&mut app, dir.path(), "outside", "again");
    let _ = app.update(Message::LoadFromDisk);
    assert_eq!(
        app.status,
        "reloaded show.json from disk; the undo history is cleared"
    );
    assert_eq!(app.summary.name, "again");
    assert!(!app.document.as_ref().unwrap().is_dirty());
}

#[test]
fn the_watch_reports_a_file_written_in_the_folder() {
    use iced::futures::StreamExt;
    let dir = tempfile::tempdir().unwrap();
    let mut changes = Box::pin(crate::watcher::watch(&(dir.path().to_owned(), true)));
    let (sender, receiver) = std::sync::mpsc::channel();
    let path = dir.path().join("show.json");
    std::thread::spawn(move || {
        let _ = sender.send(iced::futures::executor::block_on(changes.next()));
    });
    // Give the watch a moment to start before writing.
    std::thread::sleep(std::time::Duration::from_millis(100));
    std::fs::write(&path, "{}").unwrap();
    let paths = receiver
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap()
        .unwrap();
    assert!(paths.iter().any(|p| p.ends_with("show.json")), "{paths:?}");
}

#[test]
fn the_watch_does_not_report_a_file_only_read() {
    use iced::futures::StreamExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("show.json");
    std::fs::write(&path, "{}").unwrap();
    let mut changes = Box::pin(crate::watcher::watch(&(dir.path().to_owned(), true)));
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        while let Some(paths) = iced::futures::executor::block_on(changes.next()) {
            if sender.send(paths).is_err() {
                break;
            }
        }
    });
    std::thread::sleep(std::time::Duration::from_millis(100));
    // Reading it, as the editor does to compare, is no change.
    let _ = std::fs::read(&path).unwrap();
    assert!(
        receiver
            .recv_timeout(std::time::Duration::from_millis(600))
            .is_err()
    );
    std::fs::write(&path, "{\"a\": 1}").unwrap();
    assert!(
        receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .is_ok()
    );
}

/// The base value the engine has for a property of the layer at `path`.
fn base(app: &App, path: &LayerPath, property: Property) -> Option<Value> {
    let session = app.session.as_ref()?;
    let engine = lock(&session.engine);
    engine
        .explain(path, property)
        .into_iter()
        .find_map(|source| match source {
            cuelight_core::Influence::Base { value } => Some(value),
            _ => None,
        })
}

#[test]
fn a_number_typed_in_the_inspector_edits_the_show_and_undoes() {
    let (mut app, _dir) = open_copy();
    let group = LayerPath::new(cuelight_core::Root::Show, [1]);
    let _ = app.update(Message::Choose(group.clone()));
    assert_eq!(app.field(&group, Property::X), Some("32"));

    let _ = app.update(Message::Type(Property::X, "40".to_owned()));
    assert_eq!(
        app.field(&group, Property::X),
        Some("40"),
        "the field shows what is typed"
    );
    let _ = app.update(Message::Apply(Property::X));
    assert_eq!(app.status, "x = 40");
    assert_eq!(app.document.as_ref().unwrap().value()["layers"][1]["x"], 40);
    assert_eq!(base(&app, &group, Property::X), Some(Value::Number(40.0)));
    assert_eq!(app.title(), "mini* - cuelight editor");

    let _ = app.update(Message::KeyPressed(
        keyboard::Key::Character("z".into()),
        keyboard::Modifiers::CTRL,
    ));
    assert_eq!(base(&app, &group, Property::X), Some(Value::Number(32.0)));
    assert_eq!(
        app.title(),
        "mini - cuelight editor",
        "undone back to the file"
    );
    let _ = app.update(Message::KeyPressed(
        keyboard::Key::Character("Z".into()),
        keyboard::Modifiers::CTRL | keyboard::Modifiers::SHIFT,
    ));
    assert_eq!(base(&app, &group, Property::X), Some(Value::Number(40.0)));
}

#[test]
fn a_property_the_layer_does_not_write_is_added() {
    let (mut app, _dir) = open_copy();
    let floor = LayerPath::new(cuelight_core::Root::Show, [0]);
    let _ = app.update(Message::Choose(floor.clone()));
    let _ = app.update(Message::Type(Property::Opacity, "0.5".to_owned()));
    let _ = app.update(Message::Apply(Property::Opacity));
    assert_eq!(
        app.document.as_ref().unwrap().value()["layers"][0]["opacity"],
        0.5
    );
    assert_eq!(
        base(&app, &floor, Property::Opacity),
        Some(Value::Number(0.5))
    );
}

#[test]
fn what_cannot_be_read_changes_nothing() {
    let (mut app, _dir) = open_copy();
    let group = LayerPath::new(cuelight_core::Root::Show, [1]);
    let _ = app.update(Message::Choose(group.clone()));
    let before = app.document.as_ref().unwrap().text();
    let _ = app.update(Message::Type(Property::X, "forty".to_owned()));
    let _ = app.update(Message::Apply(Property::X));
    assert_eq!(app.status, "\"forty\" is not a number");
    assert_eq!(app.document.as_ref().unwrap().text(), before);
}

#[test]
fn a_property_a_binding_owns_asks_before_its_base_changes() {
    let (mut app, _dir) = open_copy();
    let dot = LayerPath::new(cuelight_core::Root::Show, [1, 0]);
    let _ = app.update(Message::Choose(dot.clone()));
    let _ = app.update(Message::Type(Property::Opacity, "0.3".to_owned()));
    let _ = app.update(Message::Apply(Property::Opacity));
    let owned = app.owned.as_ref().expect("a question");
    assert_eq!(owned.owner, "a binding on lit");
    assert!(
        !app.document.as_ref().unwrap().is_dirty(),
        "nothing changed yet"
    );
    {
        let mut ui = simulator(app.view());
        assert!(ui.find("Edit base").is_ok());
    }
    let _ = app.update(Message::KeepOwned);
    assert!(app.owned.is_none());
    assert!(!app.document.as_ref().unwrap().is_dirty());

    let _ = app.update(Message::Apply(Property::Opacity));
    let _ = app.update(Message::EditOwned);
    assert_eq!(
        app.document.as_ref().unwrap().value()["layers"][1]["children"][0]["opacity"],
        0.3
    );
}

#[test]
fn dragging_a_label_moves_the_number_in_one_undo_step() {
    let (mut app, _dir) = open_copy();
    let group = LayerPath::new(cuelight_core::Root::Show, [1]);
    let _ = app.update(Message::Choose(group.clone()));
    let _ = app.update(Message::ScrubStart(Property::X));
    // Moves faster than frames fold into one: the value follows on the
    // next frame, from where the cursor is then.
    for x in [100.0, 101.0, 120.0, 110.0] {
        let _ = app.update(Message::ScrubMove(x));
    }
    assert_eq!(base(&app, &group, Property::X), Some(Value::Number(32.0)));
    let _ = app.update(Message::ScrubApply);
    assert_eq!(base(&app, &group, Property::X), Some(Value::Number(42.0)));
    let _ = app.update(Message::ScrubEnd);
    assert_eq!(app.document.as_ref().unwrap().value()["layers"][1]["x"], 42);
    assert_eq!(app.expanded, None, "a drag does not unfold the row");

    let _ = app.update(Message::Undo);
    assert_eq!(base(&app, &group, Property::X), Some(Value::Number(32.0)));
    assert!(
        !app.document.as_ref().unwrap().is_dirty(),
        "the whole drag undone"
    );
}

#[test]
fn a_fraction_drags_by_hundredths() {
    let (mut app, _dir) = open_copy();
    let floor = LayerPath::new(cuelight_core::Root::Show, [0]);
    let _ = app.update(Message::Choose(floor.clone()));
    let _ = app.update(Message::ScrubStart(Property::Opacity));
    let _ = app.update(Message::ScrubMove(200.0));
    let _ = app.update(Message::ScrubMove(170.0));
    let _ = app.update(Message::ScrubEnd);
    assert_eq!(
        app.document.as_ref().unwrap().value()["layers"][0]["opacity"],
        0.7
    );
}

#[test]
fn a_click_on_a_label_unfolds_its_sources() {
    let (mut app, _dir) = open_copy();
    let group = LayerPath::new(cuelight_core::Root::Show, [1]);
    let _ = app.update(Message::Choose(group));
    let _ = app.update(Message::ScrubStart(Property::Y));
    let _ = app.update(Message::ScrubMove(100.0));
    let _ = app.update(Message::ScrubMove(101.0));
    let _ = app.update(Message::ScrubEnd);
    assert_eq!(app.expanded, Some(Property::Y));
    assert!(!app.document.as_ref().unwrap().is_dirty());
}

#[test]
fn a_number_a_binding_owns_is_not_dragged() {
    let (mut app, _dir) = open_copy();
    let dot = LayerPath::new(cuelight_core::Root::Show, [1, 0]);
    let _ = app.update(Message::Choose(dot));
    let _ = app.update(Message::ScrubStart(Property::Opacity));
    let _ = app.update(Message::ScrubMove(100.0));
    let _ = app.update(Message::ScrubMove(140.0));
    let _ = app.update(Message::ScrubEnd);
    assert!(app.status.contains("a binding on lit"), "{}", app.status);
    assert!(!app.document.as_ref().unwrap().is_dirty());
}

#[test]
fn a_toggle_and_a_colour_edit_the_base() {
    let (mut app, _dir) = open_copy();
    let floor = LayerPath::new(cuelight_core::Root::Show, [0]);
    let _ = app.update(Message::Choose(floor.clone()));
    let _ = app.update(Message::Put(Property::Visible, "false".to_owned()));
    assert_eq!(
        app.document.as_ref().unwrap().value()["layers"][0]["visible"],
        false
    );
    assert_eq!(
        base(&app, &floor, Property::Visible),
        Some(Value::Bool(false))
    );

    // The dot is an image: its tint, typed, then moved by a channel.
    let dot = LayerPath::new(cuelight_core::Root::Show, [1, 0]);
    let _ = app.update(Message::Choose(dot.clone()));
    let _ = app.update(Message::Type(Property::Tint, "#ff8000".to_owned()));
    let _ = app.update(Message::Apply(Property::Tint));
    let tint = || app_tint(&app);
    assert_eq!(tint(), "#FF8000");
    let _ = app.update(Message::Type(Property::Tint, "#FF800080".to_owned()));
    let _ = app.update(Message::Apply(Property::Tint));
    assert_eq!(app_tint(&app), "#FF800080");
    let _ = app.update(Message::Type(Property::Tint, "orange".to_owned()));
    let _ = app.update(Message::Apply(Property::Tint));
    assert!(app.status.contains("not a colour"), "{}", app.status);
    assert_eq!(app_tint(&app), "#FF800080");
}

fn app_tint(app: &App) -> String {
    app.document.as_ref().unwrap().value()["layers"][1]["children"][0]["tint"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn a_font_is_picked_from_the_shows_styles() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../cuelight-editor-core/tests/fixtures/typed");
    let dir = tempfile::tempdir().unwrap();
    std::fs::copy(fixture.join("show.json"), dir.path().join("show.json")).unwrap();
    std::fs::create_dir_all(dir.path().join("assets/fonts")).unwrap();
    for entry in std::fs::read_dir(fixture.join("assets/fonts")).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(
            entry.path(),
            dir.path().join("assets/fonts").join(entry.file_name()),
        )
        .unwrap();
    }
    let (mut app, _) = App::new();
    let _ = app.update(Message::Dropped(dir.path().into()));
    let a = LayerPath::new(cuelight_core::Root::Show, [0]);
    let _ = app.update(Message::Choose(a.clone()));
    {
        let session = app.session.as_ref().unwrap();
        let engine = lock(&session.engine);
        let options = app.choices(engine.show().unwrap(), &a, Property::Font);
        assert_eq!(options, Some(vec!["loud".to_owned(), "plain".to_owned()]));
    }
    let _ = app.update(Message::Put(Property::Font, "loud".to_owned()));
    assert_eq!(
        app.document.as_ref().unwrap().value()["layers"][0]["font"],
        "loud"
    );
    assert_eq!(
        base(&app, &a, Property::Font),
        Some(Value::Text("loud".to_owned()))
    );
}

#[test]
fn a_layers_other_fields_are_edited_under_its_properties() {
    let (mut app, _dir) = open_copy();
    let floor = LayerPath::new(cuelight_core::Root::Show, [0]);
    let _ = app.update(Message::Choose(floor));
    let labels: Vec<_> = app.layer_fields.iter().map(|f| f.field.label).collect();
    assert!(
        labels.contains(&"fill") && labels.contains(&"stroke width"),
        "{labels:?}"
    );
    let blend = app
        .layer_fields
        .iter()
        .find(|f| f.field.label == "blend")
        .unwrap();
    assert!(!blend.written, "the floor leaves its blend to the default");

    let floor_json = |app: &App| app.document.as_ref().unwrap().value()["layers"][0].clone();
    let _ = app.update(Message::PutField("blend", "add".to_owned()));
    assert_eq!(floor_json(&app)["blend"], "add");
    let _ = app.update(Message::TypeField("fill", "#00ff00".to_owned()));
    let _ = app.update(Message::ApplyField("fill"));
    assert_eq!(floor_json(&app)["fill"], "#00FF00");
    let _ = app.update(Message::TypeField("stroke width", "2".to_owned()));
    let _ = app.update(Message::ApplyField("stroke width"));
    assert_eq!(
        floor_json(&app)["stroke"],
        serde_json::json!({"width": 2, "color": "#FFFFFF"})
    );
    assert!(app.status.starts_with("stroke width"), "{}", app.status);

    // The dot is an image: its size, as a pair.
    let dot = LayerPath::new(cuelight_core::Root::Show, [1, 0]);
    let _ = app.update(Message::Choose(dot));
    let _ = app.update(Message::TypeField("size", "10, 12".to_owned()));
    let _ = app.update(Message::ApplyField("size"));
    assert_eq!(
        app.document.as_ref().unwrap().value()["layers"][1]["children"][0]["size"],
        serde_json::json!([10, 12])
    );
    let _ = app.update(Message::TypeField("size", "ten".to_owned()));
    let _ = app.update(Message::ApplyField("size"));
    assert!(app.status.contains("not"), "{}", app.status);
}

#[test]
fn a_default_is_not_written_and_a_written_value_resets() {
    let (mut app, _dir) = open_copy();
    let group = LayerPath::new(cuelight_core::Root::Show, [1]);
    let floor = LayerPath::new(cuelight_core::Root::Show, [0]);
    let doc = |app: &App| app.document.as_ref().unwrap().value();

    // The group writes x: it shows, and the reset takes it out.
    let _ = app.update(Message::Choose(group.clone()));
    assert_eq!(app.field(&group, Property::X), Some("32"));
    let _ = app.update(Message::Reset(Property::X));
    assert!(doc(&app)["layers"][1].get("x").is_none());
    assert_eq!(app.field(&group, Property::X), None, "the field is empty");
    assert_eq!(
        app.placeholder(Property::X),
        Some("0"),
        "the default shows greyed"
    );
    assert_eq!(app.status, "x back to its default");

    // A value set back to the default is taken out, not written down.
    let _ = app.update(Message::Choose(floor.clone()));
    let _ = app.update(Message::Type(Property::Opacity, "0.5".to_owned()));
    let _ = app.update(Message::Apply(Property::Opacity));
    assert_eq!(doc(&app)["layers"][0]["opacity"], 0.5);
    let _ = app.update(Message::Type(Property::Opacity, "1".to_owned()));
    let _ = app.update(Message::Apply(Property::Opacity));
    assert!(doc(&app)["layers"][0].get("opacity").is_none());

    // So is a field emptied with Enter.
    let _ = app.update(Message::Type(Property::Opacity, "0.5".to_owned()));
    let _ = app.update(Message::Apply(Property::Opacity));
    let _ = app.update(Message::Type(Property::Opacity, String::new()));
    let _ = app.update(Message::Apply(Property::Opacity));
    assert!(doc(&app)["layers"][0].get("opacity").is_none());

    // And a layer's other field picked back to its default.
    let _ = app.update(Message::PutField("blend", "add".to_owned()));
    assert_eq!(doc(&app)["layers"][0]["blend"], "add");
    let _ = app.update(Message::PutField("blend", "normal".to_owned()));
    assert!(doc(&app)["layers"][0].get("blend").is_none());
}

#[test]
fn a_drag_that_ends_on_the_default_leaves_it_out() {
    let (mut app, _dir) = open_copy();
    let floor = LayerPath::new(cuelight_core::Root::Show, [0]);
    let _ = app.update(Message::Choose(floor));
    let _ = app.update(Message::ScrubStart(Property::Opacity));
    for x in [200.0, 170.0] {
        let _ = app.update(Message::ScrubMove(x));
        let _ = app.update(Message::ScrubApply);
    }
    let _ = app.update(Message::ScrubMove(200.0));
    let _ = app.update(Message::ScrubEnd);
    let document = app.document.as_ref().unwrap();
    assert!(document.value()["layers"][0].get("opacity").is_none());
    assert!(!document.is_dirty(), "back where it started");
}

#[test]
fn moving_on_from_a_field_applies_it_unless_it_does_not_read() {
    let (mut app, _dir) = open_copy();
    let group = LayerPath::new(cuelight_core::Root::Show, [1]);
    let doc = |app: &App| app.document.as_ref().unwrap().value();
    let _ = app.update(Message::Choose(group.clone()));

    // Typing into y applies the x typed before it.
    let _ = app.update(Message::Type(Property::X, "40".to_owned()));
    assert!(app.is_pending(&group, Property::X));
    let _ = app.update(Message::Type(Property::Y, "20".to_owned()));
    assert_eq!(doc(&app)["layers"][1]["x"], 40);
    assert!(!app.is_pending(&group, Property::X));
    // So does a click anywhere, which is how a field is left without
    // typing elsewhere.
    let _ = app.update(Message::Type(Property::Rotation, "15".to_owned()));
    assert!(app.has_typed());
    let _ = app.update(Message::Commit);
    assert_eq!(doc(&app)["layers"][1]["rotation"], 15);
    assert_eq!(doc(&app)["layers"][1]["y"], 20, "the y waiting too");
    let _ = app.update(Message::Type(Property::Y, "21".to_owned()));
    // Picking another layer applies the y.
    let _ = app.update(Message::Choose(LayerPath::new(
        cuelight_core::Root::Show,
        [0],
    )));
    assert_eq!(doc(&app)["layers"][1]["y"], 21);

    // What does not read is not applied, stays typed, and says why.
    let dot = LayerPath::new(cuelight_core::Root::Show, [1, 0]);
    let _ = app.update(Message::Choose(dot.clone()));
    let _ = app.update(Message::Type(Property::Tint, "#FFF".to_owned()));
    let error = app.typing_error(Property::Tint).expect("a reason");
    assert!(error.contains("not a colour"), "{error}");
    let _ = app.update(Message::Type(Property::Rotation, "10".to_owned()));
    assert!(doc(&app)["layers"][1]["children"][0].get("tint").is_none());
    assert_eq!(
        app.field(&dot, Property::Tint),
        Some("#FFF"),
        "kept as typed"
    );
    let mut ui = simulator(app.view());
    assert!(
        ui.find(error.as_str()).is_ok(),
        "the reason is under its row"
    );
}

#[test]
fn the_stage_zooms_and_fits_again() {
    let (mut app, _) = App::new();
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../cuelight-editor-core/tests/fixtures/mini"
    );
    let _ = app.update(Message::Dropped(dir.into()));
    assert_eq!(app.zoom, Zoom::Fit);
    let mut ui = simulator(app.view());
    assert!(ui.find("Fit").is_ok());
    let _ = ui.click("100%");
    for message in ui.into_messages() {
        let _ = app.update(message);
    }
    assert_eq!(app.zoom, Zoom::Scale(1.0));
    let _ = app.update(Message::ZoomBy(Zoom::STEP));
    assert_eq!(app.zoom, Zoom::Scale(Zoom::STEP));
    {
        let mut ui = simulator(app.view());
        assert!(ui.find("125%").is_ok(), "the bar shows the scale");
    }
    let _ = app.update(Message::KeyPressed(
        keyboard::Key::Character("f".into()),
        keyboard::Modifiers::empty(),
    ));
    assert_eq!(app.zoom, Zoom::Fit);
    // Zooming out of a fit starts from the fitted scale, which the
    // layout found.
    {
        let mut ui = simulator(app.view());
        let _ = ui.find("Fit");
    }
    let fitted = app.fitted.get();
    assert!(fitted > 0.0 && fitted != 1.0, "{fitted}");
    let _ = app.update(Message::ZoomBy(1.0 / Zoom::STEP));
    assert_eq!(app.zoom, Zoom::Scale(fitted / Zoom::STEP));
}

#[test]
fn a_large_show_zooms_all_the_way() {
    let (mut app, _) = App::new();
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../cuelight-editor-core/tests/fixtures/mini"
    );
    let _ = app.update(Message::Dropped(dir.into()));
    // Only the part in view is drawn, so the show's size no longer
    // bounds the zoom.
    app.summary.size = [4000, 4000];
    let _ = app.update(Message::ZoomBy(100.0));
    assert_eq!(app.zoom, Zoom::Scale(Zoom::MAX));
}

#[test]
fn the_splits_between_the_areas_move() {
    let (mut app, _) = App::new();
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../cuelight-editor-core/tests/fixtures/mini"
    );
    let _ = app.update(Message::Dropped(dir.into()));
    let pane_grid::Node::Split { id, ratio, .. } = *app.panes.layout() else {
        panic!("the areas are split");
    };
    let _ = app.update(Message::Resized(pane_grid::ResizeEvent {
        split: id,
        ratio: 0.4,
    }));
    let pane_grid::Node::Split { ratio: now, .. } = *app.panes.layout() else {
        panic!("still split");
    };
    assert_ne!(ratio, now);
    assert!((now - 0.4).abs() < 1e-6);
    let mut ui = simulator(app.view());
    assert!(ui.find("Fit").is_ok(), "the stage is in its pane");
    assert!(
        ui.find("show, format 1").is_ok(),
        "the show's panel is in its pane"
    );
}

#[test]
fn the_library_lists_the_assets_and_where_they_are_used() {
    let (mut app, _) = App::new();
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../cuelight-editor-core/tests/fixtures/mini"
    );
    let _ = app.update(Message::Dropped(dir.into()));
    let _ = app.update(Message::Tab(Tab::Assets));
    {
        let mut ui = simulator(app.view());
        assert!(ui.find("ARTWORK").is_ok());
        assert!(ui.find("dot").is_ok());
        assert!(ui.find("png, used once").is_ok(), "its format and uses");
        assert!(ui.find("USED BY").is_err(), "nothing picked yet");
    }
    assert!(matches!(app.thumbs.as_slice(), [Some(Thumb::Image(_))]));
    let _ = app.update(Message::Select(Some(0)));
    {
        let mut ui = simulator(app.view());
        assert!(ui.find("USED BY").is_ok(), "in the inspector");
        assert!(ui.find("assets/dot.png").is_ok(), "the file");
        assert!(ui.find("image layer").is_ok());
        // A use is a link to the layer, shown where the tree has it.
        let _ = ui.click("group/dot");
        for message in ui.into_messages() {
            let _ = app.update(message);
        }
    }
    assert_eq!(app.tab, Tab::Layers);
    assert_eq!(app.selected, None);
    assert_eq!(
        app.selection,
        [LayerPath::new(cuelight_core::Root::Show, [1, 0])]
    );
    let mut ui = simulator(app.view());
    assert!(ui.find("image, group/dot").is_ok(), "the layer inspected");
}

#[test]
fn a_picked_image_is_previewed_until_a_layer_is_picked() {
    let (mut app, _) = App::new();
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../cuelight-editor-core/tests/fixtures/mini"
    );
    let _ = app.update(Message::Dropped(dir.into()));
    let _ = app.update(Message::Reveal("dot".to_owned()));
    assert_eq!((app.tab, app.selected), (Tab::Assets, Some(0)));
    {
        let mut ui = simulator(app.view());
        assert!(ui.find("image, 8 x 8 px").is_ok(), "the inspector has it");
    }
    let _ = app.update(Message::ActualSize(true));
    {
        let mut ui = simulator(app.view());
        assert!(ui.find("image, 8 x 8 px").is_ok());
    }
    // A layer picked on the stage takes the inspector back.
    let _ = app.update(Message::Pick([32.0, 14.0], Pick::default()));
    assert_eq!(app.selected, None);
    {
        let mut ui = simulator(app.view());
        assert!(ui.find("image, 8 x 8 px").is_err());
        assert!(ui.find("image, group/dot").is_ok());
    }
    // An asset the show does not have is said, not picked.
    let _ = app.update(Message::Reveal("nothing".to_owned()));
    assert_eq!(app.selected, None);
    assert!(app.status.contains("no asset"), "{}", app.status);
}

#[test]
fn a_font_shows_its_face_in_its_row_and_a_specimen_when_picked() {
    let (mut app, _) = App::new();
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../cuelight-editor-core/tests/fixtures/typed"
    );
    let _ = app.update(Message::Dropped(dir.into()));
    let _ = app.update(Message::Reveal("tiny".to_owned()));
    assert!(matches!(
        app.faces.as_slice(),
        [Some(Faces {
            sample: Face::Image { height: 3, .. },
            ..
        })]
    ));
    let mut ui = simulator(app.view());
    assert!(
        ui.find("fnt, used 2 times").is_ok(),
        "the row's format and uses"
    );
    assert!(ui.find("BITMAP, ITS OWN SIZE").is_ok(), "the specimen's");
    assert!(ui.find("loud").is_ok(), "a style using it");
    assert!(ui.find("#FF0000, border #000000 1 px").is_ok());
    assert!(ui.find("100%").is_ok());
    assert!(ui.find("800%").is_ok(), "3 pixels tall, zoomed to 24");
    assert!(ui.find("none of these characters").is_ok(), "no digits");
}

/// The JSON is the layer as the file has it: the author's own
/// layout, and nothing the engine fills in.
#[test]
fn the_json_is_the_layers_own_text() {
    let (mut app, _) = App::new();
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../cuelight-editor-core/tests/fixtures/mini"
    );
    let _ = app.update(Message::Dropped(dir.into()));
    let _ = app.update(Message::Pick([32.0, 14.0], Pick::default()));
    let session = app.session.as_ref().unwrap();
    let engine = session.engine.lock().unwrap();
    let show = engine.show().unwrap();
    let path = app.selection.last().unwrap();
    let layer = tree::layer(show, path).unwrap();
    let written = app.written(show, path, layer).unwrap();
    assert!(
        written.starts_with(r#"{ "name": "dot", "type": "image", "image": "dot""#),
        "{written}"
    );
    assert!(
        written.contains("\n  \"timelines\": ["),
        "moved left: {written}"
    );
    assert!(written.ends_with("\n}"), "{written}");
    assert!(!written.contains("\"visible\""), "no defaults: {written}");
}

#[test]
fn clicking_the_stage_picks_the_layer_that_drew_it() {
    use cuelight_core::Root;
    let (mut app, _) = App::new();
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../cuelight-editor-core/tests/fixtures/mini"
    );
    let _ = app.update(Message::Dropped(dir.into()));
    let dot = LayerPath::new(Root::Show, [1, 0]);
    let floor = LayerPath::new(Root::Show, [0]);
    // The dot is drawn at the group's place; the floor along the bottom.
    let _ = app.update(Message::Pick([32.0, 14.0], Pick::default()));
    assert_eq!(app.selection, std::slice::from_ref(&dot));
    let _ = app.update(Message::Pick([10.0, 27.0], Pick::default()));
    assert_eq!(app.selection, std::slice::from_ref(&floor));
    // Shift adds; a click on nothing clears; Escape clears.
    let _ = app.update(Message::Pick(
        [32.0, 14.0],
        Pick {
            alt: false,
            shift: true,
        },
    ));
    assert_eq!(app.selection, [floor.clone(), dot.clone()]);
    let _ = app.update(Message::Pick([1.0, 1.0], Pick::default()));
    assert!(app.selection.is_empty());
    let _ = app.update(Message::Choose(dot.clone()));
    let _ = app.update(Message::KeyPressed(
        keyboard::Key::Named(keyboard::key::Named::Escape),
        keyboard::Modifiers::empty(),
    ));
    assert!(app.selection.is_empty());
    // The tree shows both, and picking there works too.
    let mut ui = simulator(app.view());
    assert!(ui.find("SHOW").is_ok());
    assert!(ui.find("dot").is_ok());
    assert!(ui.find("group").is_ok());
}

#[test]
fn the_inspector_says_where_a_value_comes_from() {
    use cuelight_core::Root;
    let (mut app, _) = App::new();
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../cuelight-editor-core/tests/fixtures/mini"
    );
    let _ = app.update(Message::Dropped(dir.into()));
    let _ = app.update(Message::Choose(LayerPath::new(Root::Show, [1, 0])));
    {
        let mut ui = simulator(app.view());
        assert!(ui.find("PLACEMENT").is_ok());
        assert!(
            ui.find("bound to lit: 0.4").is_ok(),
            "opacity is bound, its base 1 beside what wins"
        );
        assert!(ui.find("image, group/dot").is_ok());
        assert!(ui.find("hop").is_ok(), "the timeline is listed");
    }
    // Once `go` starts the hop, y is the timeline's.
    // A seek lands exactly 0.1 s into the hop, where a clock would not.
    let _ = app.update(Message::Fire("go".to_owned()));
    let _ = app.update(Message::Seek(0.1));
    let _ = app.update(Message::Expand(Some(Property::Y)));
    let mut ui = simulator(app.view());
    assert!(
        ui.find("timeline hop: -4.5").is_ok(),
        "y is owned by the hop"
    );
    assert!(ui.find("2. base = 0").is_ok(), "the base value is last");
}

#[test]
fn a_tick_moves_the_show_and_pausing_holds_it() {
    let (mut app, _) = App::new();
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../cuelight-editor-core/tests/fixtures/mini"
    );
    let _ = app.update(Message::Dropped(dir.into()));
    let _ = app.update(Message::TogglePause);
    let start = Instant::now();
    let _ = app.update(Message::Tick(start));
    let from = app.session.as_ref().unwrap().time;
    let _ = app.update(Message::Tick(start + std::time::Duration::from_millis(500)));
    let time = app.session.as_ref().unwrap().time;
    assert!((time - from - 0.5).abs() < 1e-6, "{from} -> {time}");
    let _ = app.update(Message::TogglePause);
    let _ = app.update(Message::Tick(start + std::time::Duration::from_millis(900)));
    assert_eq!(
        app.session.as_ref().unwrap().time,
        time,
        "paused shows stand still"
    );
}

#[test]
fn seeking_and_stepping_land_where_asked() {
    let (mut app, _) = App::new();
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../cuelight-editor-core/tests/fixtures/mini"
    );
    let _ = app.update(Message::Dropped(dir.into()));
    let _ = app.update(Message::Seek(1.25));
    let session = app.session.as_ref().unwrap();
    assert!((session.time - 1.25).abs() < 1e-9);
    assert!(session.paused, "dragging the playhead pauses");
    assert_eq!(
        session.engine.lock().unwrap().time(),
        1.25,
        "the engine is there too"
    );
    let _ = app.update(Message::Step(-1.0 / 60.0));
    let session = app.session.as_ref().unwrap();
    assert!((session.time - (1.25 - 1.0 / 60.0)).abs() < 1e-9);
    let _ = app.update(Message::Step(-5.0));
    assert_eq!(
        app.session.as_ref().unwrap().time,
        0.0,
        "a step back stops at the start"
    );
}

#[test]
fn a_variable_set_by_hand_is_replayed_and_driver_steps_are_logged() {
    let (mut app, _) = App::new();
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../cuelight-editor-core/tests/fixtures/mini"
    );
    let _ = app.update(Message::Dropped(dir.into()));
    let lit = |app: &App| app.session.as_ref().unwrap().value("lit");

    // The driver lights the dot at 0.5 s; a hand lights it at 0.3 s.
    let _ = app.update(Message::Seek(0.3));
    let _ = app.update(Message::Set("lit".into(), "true".into()));
    assert_eq!(lit(&app), Some(Value::Bool(true)));
    let _ = app.update(Message::Seek(0.0));
    assert_eq!(
        lit(&app),
        Some(Value::Bool(false)),
        "before the hand set it"
    );
    let _ = app.update(Message::Seek(0.4));
    assert_eq!(
        lit(&app),
        Some(Value::Bool(true)),
        "a scrub replays the set"
    );

    // Played, the driver's steps are logged at their own instants.
    let _ = app.update(Message::Seek(0.0));
    let _ = app.update(Message::TogglePause);
    let start = Instant::now();
    let _ = app.update(Message::Tick(start));
    let _ = app.update(Message::Tick(start + std::time::Duration::from_millis(600)));
    let session = app.session.as_ref().unwrap();
    let driver: Vec<_> = session
        .happened
        .iter()
        .filter(|h| matches!(h.what, What::Driver(_)))
        .collect();
    assert_eq!(driver.len(), 2, "{:?}", session.happened);
    assert_eq!(driver[0].at, 0.5);
    assert!(matches!(&driver[0].what, What::Driver(Step::Trigger { trigger }) if trigger == "go"));
    assert!(matches!(&driver[1].what, What::Driver(Step::Set { .. })));
}

#[test]
fn keys_presses_and_buttons_reach_the_show() {
    let (mut app, _) = App::new();
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../cuelight-editor-core/tests/fixtures/mini"
    );
    let _ = app.update(Message::Dropped(dir.into()));
    assert!(app.inputs.triggers.contains("go"));
    assert_eq!(app.inputs.keys.get(" ").map(String::as_str), Some("go"));

    // The show maps Space to `go`, so Space fires it rather than pausing.
    let _ = app.update(Message::KeyPressed(
        keyboard::Key::Named(keyboard::key::Named::Space),
        keyboard::Modifiers::empty(),
    ));
    let session = app.session.as_ref().unwrap();
    assert!(
        session.paused,
        "an input by hand leaves a paused show paused"
    );
    assert!(matches!(session.happened.back().map(|h| &h.what), Some(What::Fired(t)) if t == "go"));

    // With Ctrl, the editor's own Space plays.
    let _ = app.update(Message::KeyPressed(
        keyboard::Key::Named(keyboard::key::Named::Space),
        keyboard::Modifiers::CTRL,
    ));
    assert!(!app.session.as_ref().unwrap().paused);

    // A press on the dot fires `go`; one on the floor fires nothing.
    let _ = app.update(Message::Press([32.0, 14.0]));
    let _ = app.update(Message::Press([10.0, 27.0]));
    let fired = app
        .session
        .as_ref()
        .unwrap()
        .happened
        .iter()
        .filter(|h| matches!(&h.what, What::Fired(t) if t == "go"))
        .count();
    assert_eq!(fired, 2);

    let _ = app.update(Message::Set("lit".to_owned(), "true".to_owned()));
    assert_eq!(
        app.session.as_ref().unwrap().value("lit"),
        Some(cuelight_core::Value::Bool(true))
    );
    let _ = app.update(Message::Fire("go".to_owned()));
    let mut ui = simulator(app.view());
    assert!(
        ui.find("go  [Space]").is_ok(),
        "the trigger's button names its key"
    );
    assert!(
        ui.find("ANYWHERE").is_ok(),
        "a trigger only the show's own layers hear is listed as heard anywhere"
    );
}

#[test]
fn without_the_driver_the_show_waits_for_the_hand() {
    let (mut app, _) = App::new();
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../cuelight-editor-core/tests/fixtures/mini"
    );
    let _ = app.update(Message::Dropped(dir.into()));
    let _ = app.update(Message::Drive(false));
    // The driver sets `lit` at 0.5 s; without it nothing does.
    let _ = app.update(Message::Seek(1.0));
    assert_eq!(
        app.session.as_ref().unwrap().value("lit"),
        Some(cuelight_core::Value::Bool(false))
    );
    let _ = app.update(Message::Drive(true));
    let _ = app.update(Message::Seek(1.0));
    assert_eq!(
        app.session.as_ref().unwrap().value("lit"),
        Some(cuelight_core::Value::Bool(true))
    );
}

#[test]
fn an_audio_layer_draws_its_inspector() {
    // The sound row lists the show's sounds while the inspector holds the
    // engine: the list must not ask for the engine again.
    let Some(dashboard) = examples().map(|e| e.join("demos/car_dashboard")) else {
        eprintln!("no cuelight-examples checkout: the audio layer is not tried");
        return;
    };
    let (mut app, _) = App::new();
    let _ = app.update(Message::Dropped(dashboard));
    // Whichever of its audio layers comes first.
    let audio = app.rows.iter().find_map(|row| match row {
        Row::Layer { path, kind, .. } if *kind == "audio" => Some(path.clone()),
        _ => None,
    });
    let _ = app.update(Message::Choose(audio.expect("the dashboard has a sound")));
    let mut ui = simulator(app.view());
    assert!(ui.find("LAYER").is_ok());
    // A sound is heard or not, at its gain, from where it is between the
    // speakers; it has no place or look.
    assert!(ui.find("gain").is_ok());
    assert!(ui.find("pan").is_ok());
    assert!(ui.find("rotation").is_err());
    assert!(ui.find("blend").is_err());
}

/// The examples checkout, beside this repository or where
/// `CUELIGHT_EXAMPLES` says.
pub(super) fn examples() -> Option<std::path::PathBuf> {
    let dir = std::env::var_os("CUELIGHT_EXAMPLES")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../cuelight-examples")
        });
    dir.join("examples.json").exists().then_some(dir)
}

#[test]
fn the_top_bar_names_the_show_by_its_folder_or_file() {
    assert_eq!(
        short_source("/home/me/shows/car_dashboard"),
        "car_dashboard"
    );
    assert_eq!(
        short_source("/home/me/shows/clock/show.json"),
        "clock/show.json"
    );
    assert_eq!(short_source("C:\\shows\\deck.cuelight"), "deck.cuelight");
    assert_eq!(short_source("deck.cuelight"), "deck.cuelight");
    assert_eq!(short_source("show.json"), "show.json");
}

#[test]
fn a_sound_is_played_from_its_preview_or_says_why_not() {
    let Some(dashboard) = examples().map(|e| e.join("demos/car_dashboard")) else {
        eprintln!("no cuelight-examples checkout: the sound preview is not tried");
        return;
    };
    let (mut app, _) = App::new();
    let _ = app.update(Message::Dropped(dashboard));
    // As started with --silent, whatever this machine has.
    app.audio = None;
    let _ = app.update(Message::Tab(Tab::Assets));
    let sound = app
        .library
        .iter()
        .position(|a| a.kind == cuelight_editor_core::assets::Kind::Sound);
    let _ = app.update(Message::Select(sound));
    let mut ui = simulator(app.view());
    // The list has no play button; the preview, without a sound output
    // here, says why there is none.
    assert!(ui.find("play").is_err());
    assert!(
        ui.find("No sound output: the editor was started with --silent, or found no sound device.")
            .is_ok()
    );
}

#[test]
fn the_shows_keys_variables_and_dots_are_set_from_its_inspector() {
    use cuelight_editor_core::lists::List;
    let (mut app, _) = App::new();
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../cuelight-editor-core/tests/fixtures/mini"
    );
    let _ = app.update(Message::Dropped(dir.into()));
    let show = |app: &App| app.document.as_ref().unwrap().value();

    // A key added on the empty row, then renamed by leaving its name.
    let _ = app.update(Message::TypeListName(List::Keys, None, "Enter".into()));
    let _ = app.update(Message::TypeListValue(List::Keys, None, "go".into()));
    let _ = app.update(Message::ApplyList(List::Keys, None));
    assert_eq!(show(&app)["input"]["keys"]["Enter"], "go");
    let engine = lock(&app.session.as_ref().unwrap().engine);
    assert_eq!(engine.show().unwrap().input.keys["Enter"], "go");
    drop(engine);
    // The fixture has the space bar already: a rename onto it is refused.
    let _ = app.update(Message::TypeListName(
        List::Keys,
        Some("Enter".into()),
        " ".into(),
    ));
    let _ = app.update(Message::Commit);
    assert_eq!(app.status, "\" \" is there already");
    let _ = app.update(Message::TypeListName(
        List::Keys,
        Some("Enter".into()),
        "x".into(),
    ));
    let _ = app.update(Message::Commit);
    assert_eq!(show(&app)["input"]["keys"]["x"], "go");
    assert!(show(&app)["input"]["keys"].get("Enter").is_none());

    // A variable, its starting value typed as a number.
    let _ = app.update(Message::TypeListName(List::Variables, None, "speed".into()));
    let _ = app.update(Message::TypeListValue(List::Variables, None, "12".into()));
    let _ = app.update(Message::ApplyList(List::Variables, None));
    assert_eq!(show(&app)["variables"]["speed"], 12);
    let _ = app.update(Message::RemoveListRow(List::Variables, "speed".into()));
    assert!(
        show(&app)
            .get("variables")
            .is_none_or(|v| v.get("speed").is_none())
    );

    // The dots pass is on while any of its settings is, and goes with the
    // last of them.
    let _ = app.update(Message::PutField("dot size", "0.5".into()));
    assert_eq!(
        show(&app)["output"]["passes"],
        serde_json::json!([{"dots": {"size": 0.5}}])
    );
    let _ = app.update(Message::ResetField("dot size"));
    assert!(
        show(&app)
            .get("output")
            .is_none_or(|o| o.get("passes").is_none()),
        "{}",
        show(&app)
    );
}

/// A show of `layers` on a 200 x 100 canvas, opened from a fresh folder.
fn open_layers(layers: &str) -> (App, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("show.json"),
        format!(
            "{{\n  \"format\": 1,\n  \"name\": \"t\",\n  \"size\": [200, 100],\n  \"layers\": [\n    {layers}\n  ]\n}}\n"
        ),
    )
    .unwrap();
    let (mut app, _) = App::new();
    let _ = app.update(Message::Dropped(dir.path().into()));
    assert!(app.status.starts_with("opened "), "{}", app.status);
    (app, dir)
}

/// Drag what `grip` holds from `from` to each of `to` in turn, a frame
/// after each, with `held`, and let go.
fn drag(app: &mut App, grip: Grip, from: [f64; 2], to: &[[f64; 2]], held: Held) {
    let _ = app.update(Message::Grab(grip, from));
    for at in to {
        let _ = app.update(Message::Drag(*at, held));
        let _ = app.update(Message::DragApply);
    }
    let _ = app.update(Message::Release);
}

const NO_SNAP: Held = Held {
    shift: false,
    ctrl: true,
};

#[test]
fn a_drag_writes_a_few_times_a_second_and_its_json_follows_on_letting_go() {
    let (mut app, _dir) = open_copy();
    let group = LayerPath::new(cuelight_core::Root::Show, [1]);
    let _ = app.update(Message::Choose(group.clone()));
    let _ = app.update(Message::Grab(Grip::Move, [32.0, 14.0]));
    let _ = simulator(app.view());
    let _ = app.update(Message::Drag([35.0, 16.0], NO_SNAP));
    let _ = app.update(Message::DragApply);
    assert_eq!(base(&app, &group, Property::X), Some(Value::Number(35.0)));
    // A frame right after the last write waits.
    let _ = app.update(Message::Drag([40.0, 16.0], NO_SNAP));
    let _ = app.update(Message::DragApply);
    assert_eq!(base(&app, &group, Property::X), Some(Value::Number(35.0)));
    // The JSON stays as the drag began while it is on.
    let _ = simulator(app.view());
    let held = app.held_json.borrow().clone().unwrap_or_default();
    assert!(held.contains(r#""x": 32"#), "{held}");
    // Letting go writes where it ends, and the JSON follows.
    let _ = app.update(Message::Release);
    assert_eq!(base(&app, &group, Property::X), Some(Value::Number(40.0)));
    let _ = simulator(app.view());
    assert!(app.held_json.borrow().is_none());
}

#[test]
fn dragging_a_layer_moves_it_in_one_undo_step() {
    let (mut app, _dir) = open_copy();
    let group = LayerPath::new(cuelight_core::Root::Show, [1]);
    let _ = app.update(Message::Choose(group.clone()));
    let _ = app.update(Message::Grab(Grip::Move, [32.0, 14.0]));
    let _ = app.update(Message::Drag([35.0, 16.4], NO_SNAP));
    assert_eq!(base(&app, &group, Property::X), Some(Value::Number(32.0)));
    let _ = app.update(Message::DragApply);
    // Whole pixels: the place follows the pointer, rounded.
    assert_eq!(base(&app, &group, Property::X), Some(Value::Number(35.0)));
    assert_eq!(base(&app, &group, Property::Y), Some(Value::Number(16.0)));
    let _ = app.update(Message::Drag([42.0, 21.0], NO_SNAP));
    let _ = app.update(Message::Release);
    let doc = app.document.as_ref().unwrap().value();
    assert_eq!(doc["layers"][1]["x"], 42);
    assert_eq!(doc["layers"][1]["y"], 21);
    assert!(app.grab.is_none());

    let _ = app.update(Message::Undo);
    assert_eq!(base(&app, &group, Property::X), Some(Value::Number(32.0)));
    assert!(
        !app.document.as_ref().unwrap().is_dirty(),
        "the whole drag undone"
    );
}

#[test]
fn a_drag_back_to_where_it_began_leaves_nothing() {
    let (mut app, _dir) = open_copy();
    let floor = LayerPath::new(cuelight_core::Root::Show, [0]);
    let _ = app.update(Message::Choose(floor.clone()));
    let _ = app.update(Message::Grab(Grip::Move, [10.0, 27.0]));
    let _ = app.update(Message::Drag([15.0, 27.0], NO_SNAP));
    let _ = app.update(Message::DragApply);
    assert_eq!(base(&app, &floor, Property::X), Some(Value::Number(5.0)));
    let _ = app.update(Message::Drag([10.0, 27.0], NO_SNAP));
    let _ = app.update(Message::Release);
    let document = app.document.as_ref().unwrap();
    assert!(
        document.value()["layers"][0].get("x").is_none(),
        "the default is not written"
    );
    assert!(document.value()["layers"][0].get("y").is_none());
    assert!(!document.is_dirty(), "back where it started");
}

#[test]
fn a_layer_in_a_turned_scaled_group_moves_in_the_groups_units() {
    let (mut app, _dir) = open_layers(
        r##"{"name": "g", "type": "group", "x": 100, "y": 50, "rotation": 90, "scale": 2,
      "children": [{"name": "r", "type": "shape", "x": 5, "shape": {"rect": [0, 0, 10, 10]}, "fill": "#FFFFFF"}]}"##,
    );
    let r = LayerPath::new(cuelight_core::Root::Show, [0, 0]);
    let _ = app.update(Message::Choose(r.clone()));
    // Down the canvas is along the group's x, at half the distance.
    drag(&mut app, Grip::Move, [90.0, 65.0], &[[90.0, 85.0]], NO_SNAP);
    let doc = app.document.as_ref().unwrap().value();
    assert_eq!(doc["layers"][0]["children"][0]["x"], 15);
    assert!(doc["layers"][0]["children"][0].get("y").is_none());
}

#[test]
fn the_arrow_keys_nudge_the_picked_layer() {
    let (mut app, _dir) = open_copy();
    let group = LayerPath::new(cuelight_core::Root::Show, [1]);
    let _ = app.update(Message::Choose(group.clone()));
    let key = |named| keyboard::Key::Named(named);
    let _ = app.update(Message::KeyPressed(
        key(keyboard::key::Named::ArrowRight),
        keyboard::Modifiers::empty(),
    ));
    assert_eq!(base(&app, &group, Property::X), Some(Value::Number(33.0)));
    let _ = app.update(Message::KeyPressed(
        key(keyboard::key::Named::ArrowDown),
        keyboard::Modifiers::SHIFT,
    ));
    assert_eq!(base(&app, &group, Property::Y), Some(Value::Number(24.0)));
    // A key is one step.
    let _ = app.update(Message::Undo);
    assert_eq!(base(&app, &group, Property::Y), Some(Value::Number(14.0)));
    assert_eq!(base(&app, &group, Property::X), Some(Value::Number(33.0)));
    // With nothing picked the arrows are the show's, or nothing.
    let _ = app.update(Message::Deselect);
    let before = app.document.as_ref().unwrap().text();
    let _ = app.update(Message::KeyPressed(
        key(keyboard::key::Named::ArrowLeft),
        keyboard::Modifiers::empty(),
    ));
    assert_eq!(app.document.as_ref().unwrap().text(), before);
}

const RECT: &str = r##"{"name": "r", "type": "shape", "x": 10, "y": 10, "shape": {"rect": [0, 0, 20, 10]}, "fill": "#FFFFFF"}"##;

#[test]
fn a_corner_scales_and_shift_keeps_proportions() {
    let (mut app, _dir) = open_layers(RECT);
    let r = LayerPath::new(cuelight_core::Root::Show, [0]);
    let _ = app.update(Message::Choose(r.clone()));
    drag(
        &mut app,
        Grip::Scale,
        [30.0, 20.0],
        &[[50.0, 25.0]],
        Held::default(),
    );
    let doc = |app: &App| app.document.as_ref().unwrap().value()["layers"][0].clone();
    assert_eq!(doc(&app)["scale_x"], 2);
    assert_eq!(doc(&app)["scale_y"], 1.5);
    let _ = app.update(Message::Undo);
    let shift = Held {
        shift: true,
        ctrl: false,
    };
    drag(&mut app, Grip::Scale, [30.0, 20.0], &[[50.0, 30.0]], shift);
    assert_eq!(doc(&app)["scale_x"], 2);
    assert_eq!(doc(&app)["scale_y"], 2);
}

const CLIPPED: &str = r##"{"name": "g", "type": "group", "x": 20, "y": 10, "clip": {"rect": [0, 0, 100, 50], "radius": 6},
      "children": [{"name": "r", "type": "shape", "shape": {"rect": [0, 0, 200, 100]}, "fill": "#FFFFFF"}]}"##;

#[test]
fn a_clip_corner_resizes_the_clip_in_one_undo_step() {
    let (mut app, _dir) = open_layers(CLIPPED);
    let g = LayerPath::new(cuelight_core::Root::Show, [0]);
    let _ = app.update(Message::Choose(g.clone()));
    let before = app.document.as_ref().unwrap().text();
    // The bottom right corner, by whole pixels; the top left stays.
    let corner = Grip::Clip(ClipHandle::Side([1, 1]));
    drag(
        &mut app,
        corner,
        [120.0, 60.0],
        &[[110.0, 55.0], [130.4, 69.6]],
        Held::default(),
    );
    let text = app.document.as_ref().unwrap().text();
    assert!(
        text.contains(r#""clip": {"rect": [0, 0, 110, 60], "radius": 6}"#),
        "{text}"
    );
    // Where the group is and how it is scaled are not touched.
    let doc = app.document.as_ref().unwrap().value();
    assert_eq!(doc["layers"][0]["x"], 20);
    assert!(doc["layers"][0].get("scale_x").is_none());
    let _ = app.update(Message::Undo);
    assert_eq!(app.document.as_ref().unwrap().text(), before);
    // The left side, past the right: the rect turns round.
    drag(
        &mut app,
        Grip::Clip(ClipHandle::Side([-1, 0])),
        [20.0, 35.0],
        &[[140.0, 35.0]],
        Held::default(),
    );
    let doc = app.document.as_ref().unwrap().value();
    assert_eq!(
        doc["layers"][0]["clip"],
        serde_json::json!({"rect": [100, 0, 20, 50], "radius": 6})
    );
}

#[test]
fn a_circle_clip_moves_by_its_centre() {
    let (mut app, _dir) = open_layers(
        r##"{"name": "g", "type": "group", "x": 50, "y": 50, "scale": 2, "clip": {"circle": [0, 0, 20]}, "children": []}"##,
    );
    let _ = app.update(Message::Choose(LayerPath::new(
        cuelight_core::Root::Show,
        [0],
    )));
    // On the canvas twice what it is in the group.
    drag(
        &mut app,
        Grip::Clip(ClipHandle::Centre),
        [50.0, 50.0],
        &[[60.0, 44.0]],
        Held::default(),
    );
    let doc = app.document.as_ref().unwrap().value();
    assert_eq!(
        doc["layers"][0]["clip"],
        serde_json::json!({"circle": [5, -3, 20]})
    );
    drag(
        &mut app,
        Grip::Clip(ClipHandle::Rim),
        [100.0, 44.0],
        &[[120.0, 44.0]],
        Held::default(),
    );
    let doc = app.document.as_ref().unwrap().value();
    assert_eq!(
        doc["layers"][0]["clip"],
        serde_json::json!({"circle": [5, -3, 30]})
    );
}

#[test]
fn the_knob_turns_and_shift_turns_in_steps() {
    let (mut app, _dir) = open_layers(RECT);
    let r = LayerPath::new(cuelight_core::Root::Show, [0]);
    let _ = app.update(Message::Choose(r.clone()));
    drag(
        &mut app,
        Grip::Turn,
        [20.0, 10.0],
        &[[10.0, 20.0]],
        Held::default(),
    );
    assert_eq!(
        base(&app, &r, Property::Rotation),
        Some(Value::Number(90.0))
    );
    let _ = app.update(Message::Undo);
    // 40 degrees down, in steps of 15.
    let shift = Held {
        shift: true,
        ctrl: false,
    };
    let to = [
        10.0 + 10.0 * 40f64.to_radians().cos(),
        10.0 + 10.0 * 40f64.to_radians().sin(),
    ];
    drag(&mut app, Grip::Turn, [20.0, 10.0], &[to], shift);
    assert_eq!(
        base(&app, &r, Property::Rotation),
        Some(Value::Number(45.0))
    );
}

#[test]
fn a_moving_layer_snaps_to_edges_and_shows_the_guides() {
    let (mut app, _dir) = open_layers(&format!(
        "{RECT},\n    {}",
        r##"{"name": "other", "type": "shape", "x": 100, "y": 60, "shape": {"rect": [0, 0, 20, 20]}, "fill": "#FFFFFF"}"##
    ));
    let r = LayerPath::new(cuelight_core::Root::Show, [0]);
    let _ = app.update(Message::Choose(r.clone()));
    // Its right edge 2 short of the other's left: it lines up, and the
    // stage shows the line.
    let _ = app.update(Message::Grab(Grip::Move, [20.0, 15.0]));
    let _ = app.update(Message::Drag([88.0, 33.0], Held::default()));
    let _ = app.update(Message::DragApply);
    assert_eq!(app.grab.as_ref().unwrap().guides, [Some(100.0), None]);
    let _ = app.update(Message::Release);
    assert_eq!(base(&app, &r, Property::X), Some(Value::Number(80.0)));
    assert_eq!(base(&app, &r, Property::Y), Some(Value::Number(28.0)));
    let _ = app.update(Message::Undo);
    // With Ctrl it goes where the pointer does.
    drag(&mut app, Grip::Move, [20.0, 15.0], &[[88.0, 33.0]], NO_SNAP);
    assert_eq!(base(&app, &r, Property::X), Some(Value::Number(78.0)));
    // The canvas's middle: the box's middle at 100.
    let _ = app.update(Message::Undo);
    drag(
        &mut app,
        Grip::Move,
        [20.0, 15.0],
        &[[99.0, 15.0]],
        Held::default(),
    );
    assert_eq!(base(&app, &r, Property::X), Some(Value::Number(90.0)));
}

#[test]
fn a_drag_on_what_a_timeline_owns_asks_when_it_lets_go() {
    let (mut app, _dir) = open_copy();
    let dot = LayerPath::new(cuelight_core::Root::Show, [1, 0]);
    // Before the hop runs, its timeline owns nothing: the drag writes.
    let _ = app.update(Message::Choose(dot.clone()));
    drag(&mut app, Grip::Move, [32.0, 14.0], &[[37.0, 14.0]], NO_SNAP);
    assert!(app.owned.is_none());
    assert_eq!(base(&app, &dot, Property::X), Some(Value::Number(5.0)));
    let _ = app.update(Message::Undo);
    // The hop runs: its timeline owns the dot's y now.
    let _ = app.update(Message::Fire("go".to_owned()));
    drag(&mut app, Grip::Move, [32.0, 14.0], &[[37.0, 14.0]], NO_SNAP);
    assert!(
        !app.document.as_ref().unwrap().is_dirty(),
        "nothing written yet"
    );
    let owned = app.owned.as_ref().expect("a question");
    assert_eq!(owned.owner, "timeline hop");
    assert_eq!(owned.names(), "x and y");
    {
        let mut ui = simulator(app.view());
        assert!(ui.find("Edit base").is_ok());
    }
    let _ = app.update(Message::EditOwned);
    let doc = app.document.as_ref().unwrap().value();
    assert_eq!(doc["layers"][1]["children"][0]["x"], 5);
    let _ = app.update(Message::Undo);
    assert!(
        !app.document.as_ref().unwrap().is_dirty(),
        "one step, like the drag"
    );
}

#[test]
fn a_theme_picked_in_the_top_bar_wins_over_the_systems() {
    let (mut app, _) = App::new();
    let _ = app.update(Message::Mode(iced::theme::Mode::Dark));
    assert!(window_theme(&app).is_none(), "the system's is left to iced");
    let _ = app.update(Message::PickTheme(Some(iced::theme::Mode::Light)));
    assert_eq!(
        window_theme(&app),
        Some(<Theme as iced::theme::Base>::default(
            iced::theme::Mode::Light
        ))
    );
    assert_eq!(
        theme(&app),
        <Theme as iced::theme::Base>::default(iced::theme::Mode::Light)
    );
    let _ = app.update(Message::PickTheme(None));
    assert_eq!(
        theme(&app),
        <Theme as iced::theme::Base>::default(iced::theme::Mode::Dark)
    );
}

/// The mini fixture copied into `show`, and an app keeping its journal
/// in `journal` opened on it.
fn open_journaled(show: &std::path::Path, journal: &std::path::Path) -> App {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../cuelight-editor-core/tests/fixtures/mini");
    for name in ["show.json", "test-driver.json", "assets/dot.png"] {
        let to = show.join(name);
        if !to.exists() {
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::copy(fixture.join(name), to).unwrap();
        }
    }
    let (mut app, _) = App::new();
    app.journal.folder = Some(journal.to_owned());
    let _ = app.update(Message::Dropped(show.into()));
    app
}

/// The entry the journal holds for the open show.
fn entry(app: &App) -> Option<cuelight_editor_core::journal::Entry> {
    use cuelight_editor_core::journal;
    journal::read(
        app.journal.folder.as_ref().unwrap(),
        &journal::key(app.origin.as_ref().unwrap()),
    )
}

/// Move the group to `x`, as typed in the inspector.
fn move_group(app: &mut App, x: &str) {
    let group = LayerPath::new(cuelight_core::Root::Show, [1]);
    let _ = app.update(Message::Choose(group));
    let _ = app.update(Message::Type(Property::X, x.to_owned()));
    let _ = app.update(Message::Apply(Property::X));
}

#[test]
fn edits_lost_in_a_crash_are_restored_on_opening_again() {
    let (show, journal) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut app = open_journaled(show.path(), journal.path());
    assert!(entry(&app).is_none(), "nothing to keep before an edit");
    move_group(&mut app, "40");
    let edited = app.document.as_ref().unwrap().text();
    assert_eq!(entry(&app).unwrap().text, edited);
    drop(app);

    let mut app = open_journaled(show.path(), journal.path());
    assert_eq!(app.title(), "mini - cuelight editor");
    let mut ui = simulator(app.view());
    assert!(
        ui.find("This show has unsaved edits from moments ago.")
            .is_ok()
    );
    let _ = ui.click("Restore");
    for message in ui.into_messages() {
        let _ = app.update(message);
    }
    assert!(app.journal.offer.is_none());
    assert_eq!(app.document.as_ref().unwrap().text(), edited);
    assert_eq!(app.title(), "mini* - cuelight editor");
    let group = LayerPath::new(cuelight_core::Root::Show, [1]);
    assert_eq!(base(&app, &group, Property::X), Some(Value::Number(40.0)));

    // One undo goes back to the file, which leaves nothing to keep.
    let _ = app.update(Message::Undo);
    assert_eq!(app.title(), "mini - cuelight editor");
    assert!(entry(&app).is_none());
}

#[test]
fn discarded_edits_are_gone() {
    let (show, journal) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut app = open_journaled(show.path(), journal.path());
    move_group(&mut app, "40");
    let mut app = open_journaled(show.path(), journal.path());
    let opened = app.document.as_ref().unwrap().text();
    let _ = app.update(Message::DiscardEdits);
    assert_eq!(app.status, "discarded the unsaved edits");
    assert!(entry(&app).is_none());
    assert_eq!(app.document.as_ref().unwrap().text(), opened);
    let app = open_journaled(show.path(), journal.path());
    assert!(app.journal.offer.is_none());
}

#[test]
fn a_save_clears_the_journal() {
    let (show, journal) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut app = open_journaled(show.path(), journal.path());
    move_group(&mut app, "40");
    assert!(entry(&app).is_some());
    let _ = app.update(Message::Save);
    assert!(app.status.starts_with("saved "), "{}", app.status);
    assert!(entry(&app).is_none());
    let app = open_journaled(show.path(), journal.path());
    assert!(app.journal.offer.is_none());
}

#[test]
fn edits_to_a_show_that_changed_since_say_so() {
    let (show, journal) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut app = open_journaled(show.path(), journal.path());
    move_group(&mut app, "40");
    let file = show.path().join("show.json");
    let text = std::fs::read_to_string(&file).unwrap();
    std::fs::write(
        &file,
        text.replace("\"name\": \"mini\"", "\"name\": \"outside\""),
    )
    .unwrap();

    let mut app = open_journaled(show.path(), journal.path());
    assert!(app.journal.offer.as_ref().unwrap().changed);
    {
        let mut ui = simulator(app.view());
        assert!(
            ui.find(
                "This show has unsaved edits from moments ago, but it changed since; restoring them drops that change."
            )
            .is_ok()
        );
    }
    let _ = app.update(Message::RestoreEdits);
    assert_eq!(app.summary.name, "mini", "the edits' own text is back");
    assert!(app.document.as_ref().unwrap().is_dirty());
}

#[test]
fn a_font_style_is_picked_under_its_font_and_set_like_a_layer() {
    let (mut app, _) = App::new();
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../cuelight-editor-core/tests/fixtures/typed"
    );
    let _ = app.update(Message::Dropped(dir.into()));
    let _ = app.update(Message::Tab(Tab::Assets));
    {
        let mut ui = simulator(app.view());
        assert!(
            ui.find("plain").is_ok() && ui.find("loud").is_ok(),
            "styles under their font"
        );
    }
    let style =
        |app: &App, name: &str| app.document.as_ref().unwrap().value()["fonts"][name].clone();

    let _ = app.update(Message::PickStyle("plain".into()));
    assert_eq!(app.field_text("file"), Some("tiny"));
    let _ = app.update(Message::PutField("color", "#FF0000".into()));
    assert_eq!(style(&app, "plain")["color"], "#FF0000");
    let engine = lock(&app.session.as_ref().unwrap().engine);
    assert_eq!(engine.show().unwrap().fonts["plain"].color, "#FF0000");
    drop(engine);

    // A border's width alone starts it with a colour, which it needs;
    // taking the colour out takes the border out.
    let _ = app.update(Message::PutField("border width", "2".into()));
    assert_eq!(
        style(&app, "plain")["border"],
        serde_json::json!({"width": 2, "color": "#000000"})
    );
    let _ = app.update(Message::ResetField("border"));
    assert!(style(&app, "plain").get("border").is_none());

    // A new style is named after its font and picked.
    let _ = app.update(Message::AddStyle("tiny".into()));
    assert_eq!(app.style.as_deref(), Some("tiny"));
    assert_eq!(style(&app, "tiny")["file"], "tiny");
    let _ = app.update(Message::AddStyle("tiny".into()));
    assert_eq!(app.style.as_deref(), Some("tiny_2"));

    // A rename takes the text layers using the style along, as one step;
    // a name another style has is refused.
    let users = |app: &App, name: &str| {
        let text = app.document.as_ref().unwrap().text();
        text.matches(&format!("\"font\": \"{name}\"")).count()
    };
    let using_plain = users(&app, "plain");
    assert!(using_plain > 0, "the fixture uses plain");
    let _ = app.update(Message::PickStyle("plain".into()));
    let _ = app.update(Message::TypeStyleName("loud".into()));
    let _ = app.update(Message::ApplyStyleName);
    assert_eq!(app.status, "there is a font style loud already");
    let _ = app.update(Message::TypeStyleName("body".into()));
    let _ = app.update(Message::ApplyStyleName);
    assert_eq!(app.style.as_deref(), Some("body"));
    assert_eq!(users(&app, "body"), using_plain);
    assert_eq!(users(&app, "plain"), 0);
    assert!(style(&app, "plain").is_null());
    let _ = app.update(Message::Undo);
    assert_eq!(users(&app, "plain"), using_plain, "one step back");
    // A style in use stays; one nothing uses goes.
    let _ = app.update(Message::PickStyle("plain".into()));
    let _ = app.update(Message::RemoveStyle);
    assert!(!style(&app, "plain").is_null(), "in use");
    let _ = app.update(Message::PickStyle("tiny_2".into()));
    let _ = app.update(Message::RemoveStyle);
    assert!(style(&app, "tiny_2").is_null());
    assert_eq!(app.style, None);
}

#[test]
fn closing_with_unsaved_edits_asks_first() {
    let (mut app, dir) = open_copy();
    // Nothing unsaved: nothing to ask.
    let _ = app.update(Message::CloseRequested);
    assert!(!app.closing);

    let _ = app.update(Message::PutField("background", "#102030".into()));
    assert!(app.document.as_ref().unwrap().is_dirty());
    let _ = app.update(Message::CloseRequested);
    assert!(app.closing);
    {
        let mut ui = simulator(app.view());
        assert!(ui.find("The show has unsaved edits.").is_ok());
        assert!(ui.find("Don't save").is_ok());
    }
    let _ = app.update(Message::KeepOpen);
    assert!(!app.closing, "cancel keeps the window and the edits");
    assert!(app.document.as_ref().unwrap().is_dirty());

    let _ = app.update(Message::CloseRequested);
    let _ = app.update(Message::SaveAndClose);
    assert!(
        !app.document.as_ref().unwrap().is_dirty(),
        "saved on the way out"
    );
    let saved = std::fs::read_to_string(dir.path().join("show.json")).unwrap();
    assert!(saved.contains("#102030"));
}

/// A short WAV of silence, for a show that needs a sound.
fn silence() -> Vec<u8> {
    let samples = 800u32;
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + samples * 2).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&8000u32.to_le_bytes());
    wav.extend_from_slice(&16000u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(samples * 2).to_le_bytes());
    wav.resize(wav.len() + samples as usize * 2, 0);
    wav
}

/// A show in a temporary folder, as `show.json` says, with a font, a
/// sound and a video beside it.
fn open_with_assets(show: &str) -> (App, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let fonts = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../cuelight-editor-core/tests/fixtures/typed/assets/fonts");
    std::fs::create_dir_all(dir.path().join("assets/fonts")).unwrap();
    for name in ["tiny.fnt", "tiny.png"] {
        std::fs::copy(fonts.join(name), dir.path().join("assets/fonts").join(name)).unwrap();
    }
    std::fs::create_dir_all(dir.path().join("assets/sounds")).unwrap();
    std::fs::write(dir.path().join("assets/sounds/ding.wav"), silence()).unwrap();
    std::fs::create_dir_all(dir.path().join("assets/videos")).unwrap();
    std::fs::write(dir.path().join("assets/videos/clip.webm"), b"not decoded").unwrap();
    let dot = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../cuelight-editor-core/tests/fixtures/mini/assets/dot.png");
    std::fs::copy(dot, dir.path().join("assets/dot.png")).unwrap();
    std::fs::write(dir.path().join("show.json"), show).unwrap();
    let (mut app, _) = App::new();
    let _ = app.update(Message::Dropped(dir.path().into()));
    assert!(app.status.starts_with("opened "), "{}", app.status);
    (app, dir)
}

/// What the tree lists, a layer as its kind and name, indented under
/// its group.
fn tree_lines(app: &App) -> Vec<String> {
    app.rows
        .iter()
        .map(|row| match row {
            Row::Root { name, .. } => format!("[{name}]"),
            Row::Layer {
                name, kind, depth, ..
            } => format!("{}{kind} {name}", "  ".repeat(*depth)),
        })
        .collect()
}

fn press(app: &mut App, key: &str, modifiers: keyboard::Modifiers) {
    let key = match key {
        "Delete" => keyboard::Key::Named(keyboard::key::Named::Delete),
        "ArrowUp" => keyboard::Key::Named(keyboard::key::Named::ArrowUp),
        "ArrowDown" => keyboard::Key::Named(keyboard::key::Named::ArrowDown),
        "Escape" => keyboard::Key::Named(keyboard::key::Named::Escape),
        c => keyboard::Key::Character(c.into()),
    };
    let _ = app.update(Message::KeyPressed(key, modifiers));
}

#[test]
fn a_show_with_one_layer_of_each_kind_is_built_from_empty() {
    let (mut app, dir) = open_with_assets(
        "{\n  \"format\": 1,\n  \"name\": \"t\",\n  \"size\": [200, 100],\n  \"fonts\": {\n    \"body\": { \"file\": \"tiny\" }\n  }\n}\n",
    );
    // The menu lists every kind.
    {
        let mut ui = simulator(app.view());
        assert!(ui.find("Add layer").is_ok());
        assert!(ui.find("Move to").is_ok());
    }
    use cuelight_editor_core::layers::Kind;
    for kind in Kind::ALL {
        let _ = app.update(Message::Deselect);
        let _ = app.update(Message::AddLayer(kind));
        if kind == Kind::Path {
            // The path asks for its data, which starts as a triangle.
            assert_eq!(
                app.path_typed.as_deref(),
                Some("M 0 -26 L 26 26 L -26 26 Z")
            );
            let mut ui = simulator(app.view());
            assert!(ui.find("Add path").is_ok());
            drop(ui);
            let _ = app.update(Message::TypePath("M 0 0 L 20 0 L 10 16 Z".into()));
            let _ = app.update(Message::AddPath);
            assert_eq!(app.path_typed, None);
        }
        assert!(app.status.starts_with("added"), "{kind:?}: {}", app.status);
        assert_eq!(app.selection.len(), 1, "the new layer is picked");
    }
    assert_eq!(
        tree_lines(&app),
        [
            "[show]",
            "shape rect",
            "shape rounded_rect",
            "shape circle",
            "shape path",
            "text text",
            "digits digits",
            "image dot",
            "group group",
            "audio ding",
            "video clip",
        ]
    );
    // Each was one step.
    let _ = app.update(Message::Undo);
    assert_eq!(app.rows.len(), 10);
    let _ = app.update(Message::Redo);
    // Saved, it opens again without a word against it.
    let _ = app.update(Message::Save);
    assert!(app.status.starts_with("saved"), "{}", app.status);
    let (mut again, _) = App::new();
    let _ = again.update(Message::Dropped(dir.path().into()));
    assert!(again.status.starts_with("opened "), "{}", again.status);
    assert_eq!(again.summary.problems, Vec::<String>::new());
    assert_eq!(tree_lines(&again), tree_lines(&app));
    let show = std::fs::read_to_string(dir.path().join("show.json")).unwrap();
    assert!(
        show.contains("    {\n      \"name\": \"text\",\n      \"type\": \"text\",\n      \"x\": 100,\n      \"y\": 50,\n      \"anchor\": \"center\",\n      \"text\": \"Text\",\n      \"font\": \"body\"\n    },"),
        "{show}"
    );
}

#[test]
fn what_cannot_be_added_says_why() {
    let (mut app, _dir) = open_layers(r#"{ "name": "a", "type": "group", "children": [] }"#);
    let _ = app.update(Message::AddLayer(cuelight_editor_core::layers::Kind::Audio));
    assert_eq!(
        app.status,
        "cannot add audio: the show has no sounds to play"
    );
    let _ = app.update(Message::AddLayer(cuelight_editor_core::layers::Kind::Video));
    assert_eq!(
        app.status,
        "cannot add video: the show has no videos to show"
    );
    let _ = app.update(Message::AddLayer(cuelight_editor_core::layers::Kind::Text));
    assert_eq!(
        app.status,
        "cannot add text: the show has no fonts to write in"
    );
    assert_eq!(tree_lines(&app), ["[show]", "group a"]);
    assert!(
        !app.document.as_ref().unwrap().can_undo(),
        "nothing was written"
    );
    // Path data the show cannot read makes no path, and waits to be fixed.
    let _ = app.update(Message::AddLayer(cuelight_editor_core::layers::Kind::Path));
    let _ = app.update(Message::TypePath("M 0 0 Q".into()));
    let _ = app.update(Message::AddPath);
    assert!(app.status.starts_with("cannot add: "), "{}", app.status);
    assert_eq!(app.path_typed.as_deref(), Some("M 0 0 Q"));
    assert_eq!(tree_lines(&app), ["[show]", "group a"]);
    assert!(!app.document.as_ref().unwrap().text().contains("path"));
}

#[test]
fn a_new_layer_goes_after_the_picked_one() {
    let (mut app, _dir) = open_layers(
        r##"{ "name": "a", "type": "group", "children": [
      { "name": "a1", "type": "shape", "shape": { "rect": [0, 0, 4, 4] }, "fill": "#FFFFFF" } ] },
    { "name": "b", "type": "shape", "shape": { "rect": [0, 0, 4, 4] }, "fill": "#FFFFFF" }"##,
    );
    let _ = app.update(Message::Choose(LayerPath::new(
        cuelight_core::Root::Show,
        [0, 0],
    )));
    let _ = app.update(Message::AddLayer(
        cuelight_editor_core::layers::Kind::Circle,
    ));
    assert_eq!(
        tree_lines(&app),
        [
            "[show]",
            "group a",
            "  shape a1",
            "  shape circle",
            "shape b"
        ]
    );
    assert_eq!(
        app.selection,
        [LayerPath::new(cuelight_core::Root::Show, [0, 1])]
    );
}

#[test]
fn the_keys_delete_duplicate_group_and_reorder_the_picked_layers() {
    let (mut app, _dir) = open_layers(
        r##"{ "name": "a", "type": "shape", "shape": { "rect": [0, 0, 4, 4] }, "fill": "#FFFFFF" },
    { "name": "b", "type": "shape", "shape": { "rect": [0, 0, 4, 4] }, "fill": "#FFFFFF" },
    { "name": "c", "type": "shape", "shape": { "rect": [0, 0, 4, 4] }, "fill": "#FFFFFF" }"##,
    );
    let none = keyboard::Modifiers::default();
    let ctrl = keyboard::Modifiers::CTRL;
    let show = cuelight_core::Root::Show;
    // Shift held, a click in the tree adds to the selection.
    let _ = app.update(Message::Choose(LayerPath::new(show, [0])));
    let _ = app.update(Message::Modifiers(keyboard::Modifiers::SHIFT));
    let _ = app.update(Message::Choose(LayerPath::new(show, [2])));
    let _ = app.update(Message::Modifiers(none));
    assert_eq!(app.selection.len(), 2);

    press(&mut app, "d", ctrl);
    assert_eq!(
        tree_lines(&app),
        [
            "[show]",
            "shape a",
            "shape a_2",
            "shape b",
            "shape c",
            "shape c_2"
        ]
    );
    assert_eq!(
        app.selection,
        [LayerPath::new(show, [1]), LayerPath::new(show, [4])],
        "the copies are picked"
    );
    press(&mut app, "Delete", none);
    assert_eq!(
        tree_lines(&app),
        ["[show]", "shape a", "shape b", "shape c"]
    );
    assert!(app.selection.is_empty());
    press(&mut app, "z", ctrl);
    assert_eq!(app.rows.len(), 6, "the deleted come back with one undo");
    press(&mut app, "z", ctrl);
    assert_eq!(
        tree_lines(&app),
        ["[show]", "shape a", "shape b", "shape c"]
    );

    let _ = app.update(Message::Choose(LayerPath::new(show, [2])));
    press(&mut app, "ArrowUp", keyboard::Modifiers::ALT);
    assert_eq!(
        tree_lines(&app),
        ["[show]", "shape a", "shape c", "shape b"]
    );
    assert_eq!(app.selection, [LayerPath::new(show, [1])]);
    press(&mut app, "ArrowUp", keyboard::Modifiers::ALT);
    press(&mut app, "ArrowUp", keyboard::Modifiers::ALT);
    assert_eq!(app.status, "cannot move: already first");
    press(&mut app, "ArrowDown", keyboard::Modifiers::ALT);
    assert_eq!(
        tree_lines(&app),
        ["[show]", "shape a", "shape c", "shape b"]
    );

    // Grouped where the first was, the group picked; and back.
    let _ = app.update(Message::Modifiers(keyboard::Modifiers::CTRL));
    let _ = app.update(Message::Choose(LayerPath::new(show, [2])));
    let _ = app.update(Message::Modifiers(none));
    press(&mut app, "g", ctrl);
    assert_eq!(
        tree_lines(&app),
        ["[show]", "shape a", "group group", "  shape c", "  shape b"]
    );
    assert_eq!(app.selection, [LayerPath::new(show, [1])]);
    press(&mut app, "G", ctrl | keyboard::Modifiers::SHIFT);
    assert_eq!(
        tree_lines(&app),
        ["[show]", "shape a", "shape c", "shape b"]
    );
    assert_eq!(app.selection.len(), 2, "the children are picked");
    press(&mut app, "Escape", none);
    assert!(app.selection.is_empty());
}

#[test]
fn layers_move_into_a_group_and_into_a_scene() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("show.json"),
        r##"{
  "format": 1,
  "name": "t",
  "size": [200, 100],
  "layers": [
    { "name": "g", "type": "group", "children": [] },
    { "name": "dot", "type": "shape", "shape": { "circle": [0, 0, 4] }, "fill": "#FFFFFF" }
  ],
  "scenes": [ { "name": "play", "trigger": "play" } ]
}
"##,
    )
    .unwrap();
    let (mut app, _) = App::new();
    let _ = app.update(Message::Dropped(dir.path().into()));
    let show = cuelight_core::Root::Show;
    let _ = app.update(Message::Choose(LayerPath::new(show, [1])));
    // The menu offers the show, the scene and the group.
    let choices: Vec<String> = app.move_choices().into_iter().map(|c| c.label).collect();
    assert_eq!(choices, ["show", "group g", "scene play"]);
    let _ = app.update(Message::MoveLayers(arrange::Destination::Group(
        LayerPath::new(show, [0]),
    )));
    assert_eq!(
        tree_lines(&app),
        ["[show]", "group g", "  shape dot", "[play]"]
    );
    assert_eq!(app.selection, [LayerPath::new(show, [0, 0])]);
    let _ = app.update(Message::MoveLayers(arrange::Destination::Root(
        cuelight_core::Root::Scene(0),
    )));
    assert_eq!(
        tree_lines(&app),
        ["[show]", "group g", "[play]", "shape dot"]
    );
    assert_eq!(
        app.selection,
        [LayerPath::new(cuelight_core::Root::Scene(0), [0])]
    );
    let _ = app.update(Message::MoveLayers(arrange::Destination::Root(show)));
    assert_eq!(
        tree_lines(&app),
        ["[show]", "group g", "shape dot", "[play]"]
    );
    // A group does not go inside itself.
    let _ = app.update(Message::Choose(LayerPath::new(show, [0])));
    let _ = app.update(Message::MoveLayers(arrange::Destination::Group(
        LayerPath::new(show, [0]),
    )));
    assert_eq!(app.status, "cannot move: a group cannot go inside itself");
}

#[test]
fn text_in_a_show_without_styles_gets_one_for_its_font() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../cuelight-editor-core/tests/fixtures/typed/assets/fonts");
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("assets/fonts")).unwrap();
    for name in ["tiny.fnt", "tiny.png"] {
        std::fs::copy(
            fixture.join(name),
            dir.path().join("assets/fonts").join(name),
        )
        .unwrap();
    }
    std::fs::write(
        dir.path().join("show.json"),
        "{\n  \"format\": 1,\n  \"name\": \"t\",\n  \"size\": [200, 100]\n}\n",
    )
    .unwrap();
    let (mut app, _) = App::new();
    let _ = app.update(Message::Dropped(dir.path().into()));
    let _ = app.update(Message::AddLayer(cuelight_editor_core::layers::Kind::Text));
    assert_eq!(app.status, "added text, in a new font style tiny");
    let show = app.document.as_ref().unwrap().value();
    assert_eq!(show["fonts"]["tiny"]["file"], "tiny");
    assert_eq!(show["layers"][0]["font"], "tiny");
    assert_eq!(tree_lines(&app), ["[show]", "text text"]);
    let _ = app.update(Message::Undo);
    let show = app.document.as_ref().unwrap().value();
    assert!(
        show.get("fonts").is_none()
            && show
                .get("layers")
                .is_none_or(|l| l.as_array().is_some_and(Vec::is_empty)),
        "{show}"
    );
}

#[test]
fn a_digits_row_is_picked_anywhere_in_its_box() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("show.json"),
        r##"{"format": 1, "name": "t", "size": [200, 100], "layers": [
  {"name": "score", "type": "digits", "digits": 2, "size": [80, 50], "x": 60, "y": 25, "text": "88",
   "display": {"segments": {"style": "numeric7", "fill": "#FFFFFF"}}}
]}"##,
    )
    .unwrap();
    let (mut app, _) = App::new();
    let _ = app.update(Message::Dropped(dir.path().into()));
    // The middle of the first cell's upper half: between its segments.
    let gap = [79.0, 37.0];
    let engine = lock(&app.session.as_ref().unwrap().engine);
    assert!(
        engine.layers_at(gap).is_empty(),
        "a press there hits no segment"
    );
    drop(engine);
    let _ = app.update(Message::Pick(gap, crate::stage::Pick::default()));
    assert_eq!(
        app.selection,
        [LayerPath::new(cuelight_core::Root::Show, [0])]
    );
}

#[test]
fn the_log_is_selected_and_copied_but_not_edited() {
    use iced::widget::text_editor::{Action, Edit};
    let (mut app, _) = App::new();
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../cuelight-editor-core/tests/fixtures/mini"
    );
    let _ = app.update(Message::Dropped(dir.into()));
    for text in ["first", "second"] {
        app.session
            .as_mut()
            .unwrap()
            .log
            .push(cuelight_editor_core::log::Line {
                kind: cuelight_editor_core::log::Kind::Input,
                at: None,
                text: text.to_owned(),
            });
    }
    let _ = app.update(Message::KeepOpen);
    let lines: Vec<String> = app
        .session
        .as_ref()
        .unwrap()
        .log
        .lines()
        .map(|l| l.render())
        .collect();
    assert_eq!(app.log_view.text().trim_end(), lines.join("\n"));
    // Typing does nothing; a selection does.
    let _ = app.update(Message::LogAction(Action::Edit(Edit::Insert('x'))));
    assert_eq!(app.log_view.text().trim_end(), lines.join("\n"));
    let _ = app.update(Message::LogAction(Action::SelectAll));
    assert_eq!(
        app.log_view.selection().map(|s| s.trim_end().to_owned()),
        Some(lines.join("\n"))
    );
    let _ = app.update(Message::CopyLog);
    assert_eq!(app.status, "copied the log");
    // A new line waits while something is selected, and shows once the
    // selection is gone.
    app.session
        .as_mut()
        .unwrap()
        .log
        .push(cuelight_editor_core::log::Line {
            kind: cuelight_editor_core::log::Kind::Input,
            at: None,
            text: "a new line".to_owned(),
        });
    let _ = app.update(Message::KeepOpen);
    assert!(
        !app.log_view.text().contains("a new line"),
        "the selection is kept"
    );
    let _ = app.update(Message::LogAction(Action::Click(
        iced::Point::new(1.0, 1.0),
        iced::advanced::mouse::click::Kind::Single,
    )));
    assert!(app.log_view.text().contains("a new line"));
}

/// Three scenes, each entered by its own trigger, the first with a
/// timeline that starts on entering it and one that waits for a when.
const SCENES: &str = r##"{
  "format": 1,
  "name": "t",
  "size": [200, 100],
  "layers": [],
  "scenes": [
    { "name": "attract", "trigger": "attract", "layers": [
      { "name": "dot", "type": "shape", "shape": { "circle": [0, 0, 4] }, "fill": "#FFFFFF",
        "timelines": [
          { "name": "pulse", "autoplay": true, "tracks": [] },
          { "name": "pop", "when": { "variable": "score" }, "tracks": [] }
        ] }
    ] },
    { "name": "game", "trigger": "start", "layers": [] },
    { "name": "over", "trigger": "game_over", "layers": [] }
  ]
}
"##;

fn scene_names(app: &App) -> Vec<String> {
    cuelight_editor_core::scenes::names(app.document.as_ref().unwrap())
}

fn scene_json(app: &App, index: usize) -> serde_json::Value {
    app.document.as_ref().unwrap().value()["scenes"][index].clone()
}

fn click(app: &mut App, label: &str) {
    let mut ui = simulator(app.view());
    let _ = ui.click(label);
    for message in ui.into_messages() {
        let _ = app.update(message);
    }
}

#[test]
fn a_scene_heading_shows_the_scene_and_enters_it() {
    let (mut app, _dir) = open_with_assets(SCENES);
    let active = |app: &App| app.session.as_ref().unwrap().active_scene();
    click(&mut app, "SCENE game");
    assert_eq!(app.scene, Some(1));
    assert_eq!(active(&app).as_deref(), Some("game"));
    // The heading of the active scene is starred; the picked one is the
    // inspector's.
    click(&mut app, "SCENE attract");
    assert_eq!(active(&app).as_deref(), Some("attract"));
    let mut ui = simulator(app.view());
    for said in [
        "TRIGGERS",
        "OUTPUT",
        "ENTERING IT",
        "ENTERED",
        "dot: pulse",
        "dot: pop",
    ] {
        assert!(ui.find(said).is_ok(), "{said}");
    }
    assert!(ui.find("SCENE attract *").is_ok());
    drop(ui);
    // Where it was entered from, and by what.
    let entry = app
        .session
        .as_ref()
        .unwrap()
        .entries
        .last()
        .cloned()
        .unwrap();
    assert_eq!(
        (entry.scene.as_str(), entry.from.as_deref()),
        ("attract", Some("game"))
    );
    // A layer picked shows the layer instead; the show's heading the show.
    let dot = app.rows.iter().find_map(|row| match row {
        Row::Layer { path, name, .. } if name == "dot" => Some(path.clone()),
        _ => None,
    });
    let dot = dot.unwrap();
    let _ = app.update(Message::Choose(dot.clone()));
    assert_eq!(app.scene, None);
    // A layer picked in the tree enters its scene too.
    let _ = app.update(Message::EnterScene(1));
    assert_eq!(active(&app).as_deref(), Some("game"));
    let _ = app.update(Message::Choose(dot));
    assert_eq!(active(&app).as_deref(), Some("attract"));
    click(&mut app, "SCENE over");
    click(&mut app, "SHOW");
    assert_eq!(app.scene, None);
    assert_eq!(app.fields_of(), editing::FieldsOf::Show);
}

#[test]
fn scenes_are_added_renamed_moved_and_deleted_in_one_step_each() {
    let (mut app, _dir) = open_with_assets(SCENES);
    let undo = |app: &mut App| {
        let _ = app.update(Message::Undo);
    };
    click(&mut app, "SCENE game");

    // A new scene goes after the picked one, picked and entered.
    click(&mut app, "Add scene");
    assert_eq!(scene_names(&app), ["attract", "game", "scene", "over"]);
    assert_eq!(app.scene, Some(2));
    assert_eq!(scene_json(&app, 2)["trigger"], "scene");
    assert_eq!(
        app.session.as_ref().unwrap().active_scene().as_deref(),
        Some("scene")
    );

    // Renamed from its name field; a name another scene has is refused.
    let _ = app.update(Message::TypeField("name", "bonus".into()));
    let _ = app.update(Message::Commit);
    assert_eq!(scene_names(&app), ["attract", "game", "bonus", "over"]);
    let _ = app.update(Message::TypeField("name", "game".into()));
    let _ = app.update(Message::ApplyField("name"));
    assert_eq!(app.status, "cannot rename: another scene is called game");
    assert_eq!(scene_names(&app), ["attract", "game", "bonus", "over"]);
    let _ = app.update(Message::TypeField("name", "bonus".into()));
    let _ = app.update(Message::ApplyField("name"));

    // Moved up with Alt+Up, past the first, which a show starts in.
    let alt = keyboard::Modifiers::ALT;
    press(&mut app, "ArrowUp", alt);
    press(&mut app, "ArrowUp", alt);
    assert_eq!(scene_names(&app), ["bonus", "attract", "game", "over"]);
    assert_eq!(app.scene, Some(0));
    press(&mut app, "ArrowUp", alt);
    assert_eq!(app.status, "cannot move: already first");

    // Deleted with its layers; each step undoes on its own.
    click(&mut app, "SCENE attract");
    press(&mut app, "Delete", keyboard::Modifiers::default());
    assert_eq!(scene_names(&app), ["bonus", "game", "over"]);
    assert_eq!(app.scene, None);
    assert!(tree_lines(&app).iter().all(|line| !line.contains("dot")));
    undo(&mut app);
    assert_eq!(scene_names(&app), ["bonus", "attract", "game", "over"]);
    undo(&mut app);
    undo(&mut app);
    assert_eq!(scene_names(&app), ["attract", "game", "bonus", "over"]);
    undo(&mut app);
    undo(&mut app);
    assert_eq!(app.document.as_ref().unwrap().text(), SCENES);
}

#[test]
fn a_scenes_triggers_are_typed_row_by_row() {
    let (mut app, _dir) = open_with_assets(SCENES);
    click(&mut app, "SCENE game");
    // A second trigger, added from the empty row and applied on leaving.
    let _ = app.update(Message::TypeTrigger(None, "coin".into()));
    let _ = app.update(Message::Commit);
    assert_eq!(
        scene_json(&app, 1)["trigger"],
        serde_json::json!(["start", "coin"])
    );
    // The new trigger enters the scene.
    let _ = app.update(Message::Fire("attract".into()));
    let _ = app.update(Message::Fire("coin".into()));
    assert_eq!(
        app.session.as_ref().unwrap().active_scene().as_deref(),
        Some("game")
    );
    // Edited in place with Enter, then taken out: one name again.
    let _ = app.update(Message::TypeTrigger(Some(0), "begin".into()));
    let _ = app.update(Message::ApplyTriggers);
    assert_eq!(
        scene_json(&app, 1)["trigger"],
        serde_json::json!(["begin", "coin"])
    );
    let _ = app.update(Message::RemoveTrigger(1));
    assert_eq!(scene_json(&app, 1)["trigger"], "begin");
    let _ = app.update(Message::RemoveTrigger(0));
    assert!(scene_json(&app, 1).get("trigger").is_none());
    // Without one a later scene cannot be entered, and says so.
    let mut ui = simulator(app.view());
    assert!(
        ui.find("No trigger enters it: only the first scene is entered without one.")
            .is_ok()
    );
    drop(ui);
    let _ = app.update(Message::EnterScene(1));
    assert!(
        app.status.starts_with("no trigger enters scene game"),
        "{}",
        app.status
    );
}

#[test]
fn a_scenes_output_is_set_over_the_shows() {
    let (mut app, _dir) = open_with_assets(SCENES);
    click(&mut app, "SCENE game");
    // What it leaves out shows as the show's.
    let mode = |app: &App| {
        app.layer_fields
            .iter()
            .find(|f| f.field.label == "mode")
            .map(|f| (f.default.clone(), f.written))
    };
    assert_eq!(mode(&app), Some(("rgb".to_owned(), false)));
    let _ = app.update(Message::PutField("mode", "gray4".into()));
    assert_eq!(scene_json(&app, 1)["output"]["mode"], "gray4");
    // Even the show's own value is an override, written.
    let _ = app.update(Message::PutField("mode", "rgb".into()));
    assert_eq!(scene_json(&app, 1)["output"]["mode"], "rgb");
    let _ = app.update(Message::ResetField("mode"));
    assert!(
        scene_json(&app, 1).get("output").is_none(),
        "{}",
        scene_json(&app, 1)
    );
}

/// `from` copied into `to`, folders and all.
fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
fn red_riding_hoods_pages_are_reordered_and_entered_from_the_menu() {
    let Some(book) = examples().map(|e| e.join("demos/red_riding_hood")) else {
        eprintln!("no cuelight-examples checkout: the book's pages are not tried");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&book, dir.path());
    let (mut app, _) = App::new();
    let _ = app.update(Message::Dropped(dir.path().into()));
    assert!(app.status.starts_with("opened "), "{}", app.status);
    let menu =
        |app: &App| -> Vec<String> { app.scene_choices().into_iter().map(|c| c.name).collect() };
    let pages = menu(&app);
    assert!(pages.len() >= 3, "{pages:?}");
    // The last page picked by its heading (far down the tree, out of the
    // simulator's view) and moved up one place.
    let last = pages.last().cloned().unwrap();
    let before = pages.get(pages.len() - 2).cloned().unwrap();
    let _ = app.update(Message::PickScene(pages.len() - 1));
    assert_eq!(app.scene, Some(pages.len() - 1));
    let _ = app.update(Message::MoveScene(true));
    assert_eq!(app.scene, Some(pages.len() - 2), "{}", app.status);
    let moved = menu(&app);
    assert_eq!(moved.get(moved.len() - 2), Some(&last));
    assert_eq!(moved.last(), Some(&before));
    assert_eq!(moved, scene_names(&app));

    // Entered from the top bar's menu.
    let second = app.scene_choices().into_iter().nth(1).unwrap();
    let _ = app.update(Message::EnterScene(second.index));
    assert_eq!(
        app.session.as_ref().unwrap().active_scene().as_deref(),
        Some(second.name.as_str())
    );
}

#[test]
fn a_folded_group_hides_its_layers_until_one_is_added_inside() {
    let (mut app, _dir) = open_copy();
    let hidden = |app: &App, name: &str| {
        app.rows
            .iter()
            .zip(app.tree_lines())
            .find(|(row, _)| matches!(row, Row::Layer { name: n, .. } if n == name))
            .map(|(_, line)| line.hidden)
            .unwrap()
    };
    let group = app
        .rows
        .iter()
        .zip(app.tree_lines())
        .find(|(row, _)| matches!(row, Row::Layer { kind: "group", .. }))
        .map(|(_, line)| line.key.clone())
        .expect("the fixture has a group");
    let child = app
        .rows
        .iter()
        .find_map(|row| match row {
            Row::Layer {
                path,
                name,
                depth: 1,
                ..
            } => Some((path.clone(), name.clone())),
            _ => None,
        })
        .expect("the group has a child");
    assert!(!hidden(&app, &child.1));
    let _ = app.update(Message::ToggleFold(group.clone()));
    assert!(hidden(&app, &child.1), "folded away");
    {
        let mut ui = simulator(app.view());
        assert!(ui.find(child.1.as_str()).is_err());
    }
    // Duplicating the child (picked from the inspector's uses, say)
    // unfolds the group so the copy shows.
    let _ = app.update(Message::Choose(child.0));
    let _ = app.update(Message::DuplicateLayers);
    assert!(!hidden(&app, &child.1));
}

#[test]
fn only_the_active_scene_opens_unfolded_and_a_fold_marks_what_it_hides() {
    let Some(scenes) = examples().map(|e| e.join("features/scenes/scenes")) else {
        eprintln!("no cuelight-examples checkout: folding is not tried");
        return;
    };
    let (mut app, _) = App::new();
    let _ = app.update(Message::Dropped(scenes));
    let active = app
        .session
        .as_ref()
        .unwrap()
        .active_scene()
        .expect("a scene is active");
    let headings: Vec<(String, bool)> = app
        .rows
        .iter()
        .filter_map(|row| match row {
            Row::Root {
                root: cuelight_core::Root::Scene(_),
                name,
            } => Some((name.clone(), app.folded.contains(&format!("scene {name}")))),
            _ => None,
        })
        .collect();
    assert!(headings.len() > 1);
    for (name, folded) in &headings {
        assert_eq!(*folded, *name != active, "{name}");
    }
    // A layer of the active scene picked, then its scene folded: the
    // pick stays, and the heading says it holds it.
    let picked = app.rows.iter().find_map(|row| match row {
        Row::Layer { path, .. } if matches!(path.root, cuelight_core::Root::Scene(_)) => {
            Some(path.clone())
        }
        _ => None,
    });
    let picked = picked.expect("the active scene has a layer");
    let cuelight_core::Root::Scene(scene) = picked.root else {
        panic!("a scene's layer");
    };
    let name = app
        .rows
        .iter()
        .find_map(|row| match row {
            Row::Root {
                root: cuelight_core::Root::Scene(i),
                name,
            } if *i == scene => Some(name.clone()),
            _ => None,
        })
        .unwrap();
    let _ = app.update(Message::Choose(picked.clone()));
    let key = format!("scene {name}");
    app.folded.remove(&key);
    let _ = app.update(Message::ToggleFold(key.clone()));
    assert_eq!(app.selection, [picked]);
    let marked = app
        .tree_lines()
        .into_iter()
        .find(|line| line.key == key)
        .unwrap();
    assert!(marked.holds_picked);
}

#[test]
fn a_layer_picked_in_the_tree_enters_its_scene() {
    let Some(book) = examples().map(|e| e.join("demos/red_riding_hood")) else {
        eprintln!("no cuelight-examples checkout: picking into a scene is not tried");
        return;
    };
    let (mut app, _) = App::new();
    let _ = app.update(Message::Dropped(book));
    let the_end = app.rows.iter().find_map(|row| match row {
        Row::Layer { path, name, .. } if name == "the_end" => Some(path.clone()),
        _ => None,
    });
    let _ = app.update(Message::Choose(the_end.expect("the book ends")));
    let session = app.session.as_ref().unwrap();
    assert_eq!(session.active_scene().as_deref(), Some("page_8"));
}

#[test]
fn every_corner_of_a_turned_clipped_group_follows_the_pointer() {
    use cuelight_editor_core::placement;
    // Turned an eighth round (100, 20), seen through a 40 x 30 clip that
    // cuts its children, one of them turned its own way: the engine then
    // has no one space to give the group's box in.
    let (mut app, _dir) = open_layers(
        r##"{"name": "g", "type": "group", "x": 100, "y": 20, "rotation": 45,
      "clip": {"rect": [0, 0, 40, 30]},
      "children": [
        {"name": "r", "type": "shape", "shape": {"rect": [0, 0, 80, 60]}, "fill": "#FFFFFF"},
        {"name": "t", "type": "shape", "x": 10, "y": 10, "rotation": 10, "shape": {"rect": [0, 0, 8, 4]}, "fill": "#000000"}
      ]}"##,
    );
    let g = LayerPath::new(cuelight_core::Root::Show, [0]);
    let _ = app.update(Message::Choose(g.clone()));
    let box_of = |app: &App| {
        let engine = lock(&app.session.as_ref().unwrap().engine);
        placement::corners(&engine, &g).unwrap()
    };
    let near = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]) < 0.5;
    let start = box_of(&app);
    // The box is the clip, turned with the group.
    let expected = [[0.0, 0.0], [40.0, 0.0], [40.0, 30.0], [0.0, 30.0]].map(|[x, y]| {
        let (s, c) = std::f64::consts::FRAC_PI_4.sin_cos();
        [100.0 + x * c - y * s, 20.0 + x * s + y * c]
    });
    for (got, want) in start.iter().zip(expected) {
        assert!(near(*got, want), "{start:?}");
    }
    for corner in 0..4 {
        let grabbed = start[corner];
        let opposite = start[(corner + 2) % 4];
        // Half as far again from the corner across.
        let to = [
            opposite[0] + 1.5 * (grabbed[0] - opposite[0]),
            opposite[1] + 1.5 * (grabbed[1] - opposite[1]),
        ];
        drag(&mut app, Grip::Scale, grabbed, &[to], Held::default());
        let after = box_of(&app);
        assert!(
            near(after[corner], to),
            "corner {corner}: {after:?}, wanted {to:?}"
        );
        assert!(
            near(after[(corner + 2) % 4], opposite),
            "corner {corner}: {after:?}, across stays at {opposite:?}"
        );
        let _ = app.update(Message::Undo);
        assert!(near(box_of(&app)[corner], grabbed));
    }
}
