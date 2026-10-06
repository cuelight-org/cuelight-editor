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
    assert!(ui.find("mini (format 1)").is_ok());
    assert!(ui.find("64 x 32").is_ok());
    assert!(ui.find("Play").is_ok(), "an opened show is paused at 0");
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
fn the_zoom_stops_where_vello_stops_drawing() {
    let (mut app, _) = App::new();
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../cuelight-editor-core/tests/fixtures/mini"
    );
    let _ = app.update(Message::Dropped(dir.into()));
    // A 64 x 32 show zooms all the way.
    let _ = app.update(Message::ZoomBy(100.0));
    assert_eq!(app.zoom, Zoom::Scale(Zoom::MAX));
    // A 4000 x 4000 one on a 2x screen stops near 4096 physical pixels.
    app.summary.size = [4000, 4000];
    let _ = app.update(Message::Rescaled(2.0));
    let _ = app.update(Message::Zoom(Zoom::Scale(1.0)));
    let Zoom::Scale(scale) = app.zoom else {
        panic!("a scale");
    };
    assert!((0.45..=0.512).contains(&scale), "{scale}");
    assert!(crate::stage::drawable([
        (4000.0 * scale * 2.0).round() as u32,
        (4000.0 * scale * 2.0).round() as u32
    ]));
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
        ui.find("mini (format 1)").is_ok(),
        "the summary is in its pane"
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
    let _ = app.update(Message::Fire("go".to_owned()));
    let _ = app.update(Message::TogglePause);
    let start = Instant::now();
    let _ = app.update(Message::Tick(start));
    let _ = app.update(Message::Tick(start + std::time::Duration::from_millis(100)));
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
