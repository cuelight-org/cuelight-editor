//! Making and arranging layers from the tree: the add menu, and
//! deleting, duplicating, reordering, grouping and moving the picked
//! layers. Each is one step to undo; what it made or moved is picked
//! after it. The scenes' own buttons are here too; what they do is in
//! `scenes`.

use cuelight_core::Root;
use cuelight_editor_core::assets;
use cuelight_editor_core::document::{Part, Pointer};
use cuelight_editor_core::layers::{self, Kind, Making};
use iced::widget::Widget as _;
use iced::widget::{Column, button, pick_list, row, text, text_input};
use iced::{Element, Fill, Task};

use super::{App, Message, Row, tree};

/// A choice in the add menu: a kind, with why it cannot be made now.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Adding {
    pub kind: Kind,
    pub why_not: Option<&'static str>,
}

impl Adding {
    fn label(&self) -> String {
        match self.why_not {
            Some(why) => format!("{} ({why})", self.kind.label()),
            None => self.kind.label().to_owned(),
        }
    }
}

/// Where the move menu can put the picked layers: the show's own
/// layers, a scene's, or a group's children.
#[derive(Debug, Clone, PartialEq)]
pub enum Destination {
    Root(Root),
    Group(cuelight_core::LayerPath),
}

/// A choice in the move menu, with what it is called there.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct MoveChoice {
    pub to: Destination,
    pub label: String,
}

impl App {
    /// The menu and buttons above the tree.
    pub(super) fn arrange_bar<'a>(&'a self) -> Element<'a, Message> {
        let picked = !self.selection.is_empty();
        let making = self.making();
        let adding: Vec<Adding> = Kind::ALL
            .into_iter()
            .map(|kind| Adding {
                kind,
                // The path's data is typed after it is picked.
                why_not: layers::unavailable(kind, &making).filter(|_| kind != Kind::Path),
            })
            .collect();
        let add: Element<'a, Message> = pick_list(None::<Adding>, adding, Adding::label)
            .placeholder("Add layer")
            .on_select(|adding: Adding| Message::AddLayer(adding.kind))
            .width(Fill)
            .text_size(12)
            .padding([2, 6])
            .boxed();
        let moves = self.move_choices();
        let moving = pick_list(None::<MoveChoice>, moves, |c: &MoveChoice| c.label.clone())
            .placeholder("Move to")
            .width(Fill)
            .text_size(12)
            .padding([2, 6]);
        let moving: Element<'a, Message> = if picked {
            moving
                .on_select(|choice: MoveChoice| Message::MoveLayers(choice.to))
                .boxed()
        } else {
            moving.boxed()
        };
        let small = |label: &'a str, message: Message, on: bool| {
            button(text(label).size(12))
                .on_press_maybe(on.then_some(message))
                .padding([2, 6])
                .style(button::secondary)
        };
        let group = self.picked_group().is_some();
        // With a scene's heading picked rather than layers, Up, Down and
        // Delete are the scene's.
        let scene = !picked && self.scene.is_some();
        let (up, down, delete) = if scene {
            (
                Message::MoveScene(true),
                Message::MoveScene(false),
                Message::DeleteScene,
            )
        } else {
            (
                Message::Reorder(true),
                Message::Reorder(false),
                Message::DeleteLayers,
            )
        };
        let mut bar = Column::<Element<'a, Message>>::new()
            .spacing(4)
            .push(row![add, moving].spacing(6).boxed())
            .push(
                row![
                    small("Up", up, picked || scene),
                    small("Down", down, picked || scene),
                    small("Group", Message::GroupLayers, picked),
                    small("Ungroup", Message::Ungroup, group),
                    small("Duplicate", Message::DuplicateLayers, picked),
                    small("Delete", delete, picked || scene),
                    small("Add scene", Message::AddScene, self.document.is_some()),
                ]
                .spacing(4)
                .wrap()
                .boxed(),
            );
        // A path waits for its data.
        if let Some(path) = &self.path_typed {
            bar = bar.push(
                row![
                    text_input("SVG path data", path)
                        .on_input(Message::TypePath)
                        .on_submit(Message::AddPath)
                        .size(12)
                        .width(Fill),
                    small("Add path", Message::AddPath, true),
                    small("Cancel", Message::CancelPath, true),
                ]
                .spacing(4)
                .boxed(),
            );
        }
        bar.boxed()
    }

    /// Where the move menu offers to put the picked layers: the show,
    /// each scene, and every group not among them.
    pub(super) fn move_choices(&self) -> Vec<MoveChoice> {
        let mut out = Vec::new();
        // The names down to each row, for a group's label.
        let mut names: Vec<String> = Vec::new();
        for row_ in &self.rows {
            match row_ {
                Row::Root { root, name } => {
                    names.clear();
                    out.push(MoveChoice {
                        to: Destination::Root(*root),
                        label: match root {
                            Root::Show => "show".to_owned(),
                            Root::Scene(_) => format!("scene {name}"),
                        },
                    });
                }
                Row::Layer {
                    path,
                    name,
                    kind,
                    depth,
                } => {
                    names.truncate(*depth);
                    names.push(name.clone());
                    let inside = self.selection.iter().any(|p| tree::within(path, p));
                    if *kind == "group" && !inside {
                        let place = names.join("/");
                        out.push(MoveChoice {
                            to: Destination::Group(path.clone()),
                            label: match path.root {
                                Root::Show => format!("group {place}"),
                                Root::Scene(_) => format!("group {place} (scene)"),
                            },
                        });
                    }
                }
            }
        }
        out
    }

    /// The picked layer when it is the one layer picked and a group.
    fn picked_group(&self) -> Option<&cuelight_core::LayerPath> {
        let [path] = self.selection.as_slice() else {
            return None;
        };
        self.rows
            .iter()
            .any(|r| matches!(r, Row::Layer { path: p, kind: "group", .. } if p == path))
            .then_some(path)
    }

    /// What a new layer is made from: the canvas, the show's first font
    /// style as written, its first sound and video, and the path typed.
    fn making(&self) -> Making {
        let show = self.document.as_ref().map(|d| d.value());
        let font = show
            .as_ref()
            .and_then(|show| show.get("fonts")?.as_object()?.keys().next().cloned());
        let first = |kind: assets::Kind| {
            self.library
                .iter()
                .find(|a| a.kind == kind)
                .map(|a| a.name.clone())
        };
        Making {
            size: self.canvas_size(),
            font,
            sound: first(assets::Kind::Sound),
            video: first(assets::Kind::Video),
            artwork: self
                .library
                .iter()
                .find(|a| matches!(a.kind, assets::Kind::Image | assets::Kind::Vector))
                .map(|a| (a.name.clone(), a.kind == assets::Kind::Vector, a.size)),
            path: self.path_typed.clone().unwrap_or_default(),
        }
    }

    /// The canvas size, as the document writes it.
    fn canvas_size(&self) -> [f64; 2] {
        let show = self.document.as_ref().map(|d| d.value());
        let size = show
            .as_ref()
            .and_then(|s| s.get("size")?.as_array().cloned());
        match size.as_deref() {
            Some([w, h]) => [w.as_f64().unwrap_or(0.0), h.as_f64().unwrap_or(0.0)],
            _ => [0.0, 0.0],
        }
    }

    /// The add menu picked a kind: made at once, or for a path, its data
    /// asked for first.
    pub(super) fn add_layer(&mut self, kind: Kind) -> Task<Message> {
        if kind == Kind::Path {
            self.path_typed = Some(layers::default_path(self.canvas_size()));
            return Task::none();
        }
        self.insert_layer(kind)
    }

    /// The typed path data made into a path.
    pub(super) fn add_path(&mut self) -> Task<Message> {
        let task = self.insert_layer(Kind::Path);
        if self.status.starts_with("added") {
            self.path_typed = None;
        }
        task
    }

    /// A new layer of `kind` after the last one picked, in its list (a
    /// part's goes after its artwork), or at the end of the show's
    /// layers while nothing is picked.
    fn insert_layer(&mut self, kind: Kind) -> Task<Message> {
        let mut making = self.making();
        // Text writes in a style: a show with fonts but no style gets one
        // for its first font, in the same step as the text.
        let style = (kind == Kind::Text && making.font.is_none())
            .then(|| {
                let font = self.library.iter().find(|a| a.kind == assets::Kind::Font)?;
                Some(self.new_style(&font.name))
            })
            .flatten();
        if let Some((name, _)) = &style {
            making.font = Some(name.clone());
        }
        let layer = match layers::new_layer(kind, &making) {
            Ok(layer) => layer,
            Err(why) => {
                self.status = format!("cannot add {}: {why}", kind.label().to_lowercase());
                return Task::none();
            }
        };
        let (list, index) = match self.selection.last().cloned() {
            None => (Pointer(vec![Part::Key("layers".to_owned())]), usize::MAX),
            Some(path) => {
                let Some(mut at) = self.layer_pointer(&path) else {
                    return Task::none();
                };
                // A part's artwork is the layer the new one follows.
                let parts = Part::Key("parts".to_owned());
                if let Some(artwork) = at.0.iter().position(|p| *p == parts) {
                    at.0.truncate(artwork);
                }
                match layers::place_of(&at) {
                    Some((list, i)) => (list, i + 1),
                    None => return Task::none(),
                }
            }
        };
        let Some(document) = &mut self.document else {
            return Task::none();
        };
        let before = document.text();
        document.begin_step();
        let styled = match &style {
            Some((name, value)) => {
                super::styles::put_style(document, name, value.clone()).map_err(|e| e.to_string())
            }
            None => Ok(()),
        };
        let done = styled
            .and_then(|()| layers::insert(document, &list, index, layer))
            .map(|at| vec![at]);
        document.end_step();
        // Nothing half made: a style without its text goes too.
        if done.is_err() && document.text() != before {
            document.undo();
        }
        let said = match &style {
            Some((name, _)) => format!(
                "added {}, in a new font style {name}",
                kind.label().to_lowercase()
            ),
            None => format!("added {}", kind.label().to_lowercase()),
        };
        self.arranged(done, "add", said)
    }

    /// The document pointers of the picked layers, where the document
    /// has them.
    fn picked_pointers(&self) -> Vec<Pointer> {
        self.selection
            .iter()
            .filter_map(|path| self.layer_pointer(path))
            .collect()
    }

    pub(super) fn delete_layers(&mut self) -> Task<Message> {
        let picked = self.picked_pointers();
        if picked.is_empty() {
            return Task::none();
        }
        let Some(document) = &mut self.document else {
            return Task::none();
        };
        let done = layers::delete(document, &picked).map(|()| Vec::new());
        let said = format!("deleted {}", count(picked.len()));
        self.arranged(done, "delete", said)
    }

    pub(super) fn duplicate_layers(&mut self) -> Task<Message> {
        let picked = self.picked_pointers();
        if picked.is_empty() {
            return Task::none();
        }
        let Some(document) = &mut self.document else {
            return Task::none();
        };
        let done = layers::duplicate(document, &picked);
        let said = format!("duplicated {}", count(picked.len()));
        self.arranged(done, "duplicate", said)
    }

    pub(super) fn reorder(&mut self, up: bool) -> Task<Message> {
        let picked = self.picked_pointers();
        if picked.is_empty() {
            return Task::none();
        }
        let Some(document) = &mut self.document else {
            return Task::none();
        };
        let done = layers::reorder(document, &picked, up);
        let said = format!("moved {}", if up { "up" } else { "down" });
        self.arranged(done, "move", said)
    }

    pub(super) fn group_layers(&mut self) -> Task<Message> {
        let picked = self.picked_pointers();
        if picked.is_empty() {
            return Task::none();
        }
        let Some(document) = &mut self.document else {
            return Task::none();
        };
        let done = layers::group(document, &picked).map(|group| vec![group]);
        let said = format!("grouped {}", count(picked.len()));
        self.arranged(done, "group", said)
    }

    pub(super) fn ungroup(&mut self) -> Task<Message> {
        let Some(path) = self.picked_group().cloned() else {
            self.status = "pick one group to ungroup".to_owned();
            return Task::none();
        };
        let Some(group) = self.layer_pointer(&path) else {
            return Task::none();
        };
        let Some(document) = &mut self.document else {
            return Task::none();
        };
        let done = layers::ungroup(document, &group);
        self.arranged(done, "ungroup", "ungrouped".to_owned())
    }

    pub(super) fn move_layers(&mut self, to: Destination) -> Task<Message> {
        let picked = self.picked_pointers();
        if picked.is_empty() {
            return Task::none();
        }
        let list = match &to {
            Destination::Root(Root::Show) => Some(Pointer(vec![Part::Key("layers".to_owned())])),
            Destination::Root(Root::Scene(i)) => Some(Pointer(vec![
                Part::Key("scenes".to_owned()),
                Part::Index(*i),
                Part::Key("layers".to_owned()),
            ])),
            Destination::Group(path) => self.layer_pointer(path).map(|g| layers::children_of(&g)),
        };
        let Some(list) = list else {
            return Task::none();
        };
        let Some(document) = &mut self.document else {
            return Task::none();
        };
        let done = layers::move_into(document, &picked, &list);
        let said = format!("moved {}", count(picked.len()));
        self.arranged(done, "move", said)
    }

    /// Reload after an arrangement and pick what it gives; a show that
    /// no longer loads takes it back.
    fn arranged(
        &mut self,
        done: Result<Vec<Pointer>, String>,
        what: &str,
        said: String,
    ) -> Task<Message> {
        let picked = match done {
            Ok(picked) => picked,
            Err(why) => {
                self.status = format!("cannot {what}: {why}");
                return Task::none();
            }
        };
        let Some(text) = self.document.as_ref().map(|d| d.text()) else {
            return Task::none();
        };
        // What was typed was for layers that may be elsewhere now.
        self.typed = None;
        self.field_typed = None;
        self.expanded = None;
        self.unfolded_field = None;
        let refused = match self.reload_text(&text, true) {
            Ok(()) => {
                let picked: Vec<_> = picked.iter().filter_map(tree::path_of).collect();
                // A layer the show leaves out at load (path data it
                // cannot read, say) is not made at all.
                let left_out = picked.iter().any(|path| {
                    !self
                        .rows
                        .iter()
                        .any(|row| matches!(row, Row::Layer { path: p, .. } if p == path))
                });
                if left_out {
                    let why = self.summary.problems.last().cloned().unwrap_or_default();
                    Some(format!("cannot {what}: the show leaves it out ({why})"))
                } else {
                    self.selection = picked;
                    self.status = said;
                    None
                }
            }
            Err(error) => Some(error),
        };
        if let Some(refused) = refused {
            if let Some(document) = &mut self.document {
                document.undo();
                let text = document.text();
                let _ = self.reload_text(&text, true);
            }
            self.status = refused;
        }
        Task::none()
    }
}

/// `1 layer`, `3 layers`.
fn count(n: usize) -> String {
    if n == 1 {
        "1 layer".to_owned()
    } else {
        format!("{n} layers")
    }
}
