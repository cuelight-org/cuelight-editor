use super::*;
use cuelight_core::Value;
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
    let relay = LayerPath::new(cuelight_core::Root::Show, [5]);
    let _ = app.update(Message::Choose(relay));
    let mut ui = simulator(app.view());
    assert!(ui.find("relay").is_ok());
    assert!(ui.find("LAYER").is_ok());
    // A sound is heard or not, at its gain; it has no place or look.
    assert!(ui.find("gain").is_ok());
    assert!(ui.find("rotation").is_err());
    assert!(ui.find("blend").is_err());
}

/// The examples checkout, beside this repository or where
/// `CUELIGHT_EXAMPLES` says.
fn examples() -> Option<std::path::PathBuf> {
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
