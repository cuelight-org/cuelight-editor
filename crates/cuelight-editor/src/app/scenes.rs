//! Scenes from the tree and the inspector: adding, deleting, moving and
//! renaming one, each one step to undo; the triggers that enter it,
//! typed row by row like the show's lists; and the scene's inspector,
//! which says what entering it does and where it was entered from.

use cuelight_core::Root;
use cuelight_editor_core::scenes;
use cuelight_editor_core::session::{Instant, Session, lock};
use iced::widget::Widget as _;
use iced::widget::{Column, container, row, text, text_input};
use iced::{Element, Fill, Task};

use super::{App, Message, Row, editing};

/// A scene in the top bar's menu: entering it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SceneChoice {
    pub index: usize,
    pub name: String,
}

impl std::fmt::Display for SceneChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

/// How many of a scene's latest entries its inspector lists.
const ENTRIES_SHOWN: usize = 8;

impl App {
    /// The scenes as the top bar's menu offers them, in order.
    pub(super) fn scene_choices(&self) -> Vec<SceneChoice> {
        self.rows
            .iter()
            .filter_map(|row| match row {
                Row::Root {
                    root: Root::Scene(index),
                    name,
                } => Some(SceneChoice {
                    index: *index,
                    name: name.clone(),
                }),
                _ => None,
            })
            .collect()
    }

    /// A scene's heading clicked in the tree: its settings in the
    /// inspector, and the scene entered.
    pub(super) fn pick_scene(&mut self, index: usize) -> Task<Message> {
        self.selection.clear();
        self.selected = None;
        self.expanded = None;
        self.unfolded_field = None;
        if self.scene != Some(index) {
            self.triggers_typed.clear();
        }
        self.scene = Some(index);
        self.enter_scene(index)
    }

    /// Enter the scene at `index`, paused where the playhead is.
    pub(super) fn enter_scene(&mut self, index: usize) -> Task<Message> {
        let Some(session) = &mut self.session else {
            return Task::none();
        };
        if session.enter_scene(index, Instant::now()) {
            self.follow_scene = true;
            self.hush();
        } else {
            let name = self
                .scene_choices()
                .into_iter()
                .find(|c| c.index == index)
                .map(|c| c.name)
                .unwrap_or_default();
            self.status = format!("no trigger enters scene {name}: give it one to enter it");
        }
        Task::none()
    }

    /// A new scene after the picked one, or after the scene of the last
    /// layer picked, or at the end; picked and entered.
    pub(super) fn add_scene(&mut self) -> Task<Message> {
        let after = self.scene.or_else(|| {
            self.selection.last().and_then(|path| match path.root {
                Root::Scene(i) => Some(i),
                Root::Show => None,
            })
        });
        let Some(document) = &mut self.document else {
            return Task::none();
        };
        let at = after.map_or(usize::MAX, |i| i + 1);
        let done = scenes::add(document, at).map(Some);
        match (
            self.scenes_arranged(done, "add a scene", "added a scene"),
            self.scene,
        ) {
            (true, Some(index)) => self.enter_scene(index),
            _ => Task::none(),
        }
    }

    /// Take the picked scene out, its layers with it.
    pub(super) fn delete_scene(&mut self) -> Task<Message> {
        let (Some(index), Some(document)) = (self.scene, &mut self.document) else {
            return Task::none();
        };
        let name = scenes::names(document)
            .get(index)
            .cloned()
            .unwrap_or_default();
        let done = scenes::delete(document, index).map(|()| None);
        self.scenes_arranged(done, "delete", &format!("deleted scene {name}"));
        Task::none()
    }

    /// Move the picked scene one place up the list, or down.
    pub(super) fn move_scene(&mut self, up: bool) -> Task<Message> {
        let (Some(index), Some(document)) = (self.scene, &mut self.document) else {
            return Task::none();
        };
        let done = scenes::reorder(document, index, up).map(Some);
        let said = format!("moved the scene {}", if up { "up" } else { "down" });
        self.scenes_arranged(done, "move", &said);
        Task::none()
    }

    /// Reload after a scene was added, deleted or moved, and pick the
    /// scene it gives; a show that no longer loads takes it back.
    /// Whether it stays.
    fn scenes_arranged(
        &mut self,
        done: Result<Option<usize>, String>,
        what: &str,
        said: &str,
    ) -> bool {
        let picked = match done {
            Ok(picked) => picked,
            Err(why) => {
                self.status = format!("cannot {what}: {why}");
                return false;
            }
        };
        let Some(text) = self.document.as_ref().map(|d| d.text()) else {
            return false;
        };
        // What was typed was for a scene that may be elsewhere now; the
        // layers picked may be too.
        self.field_typed = None;
        self.triggers_typed.clear();
        self.selection.clear();
        match self.reload_text(&text, true) {
            Ok(()) => {
                self.scene = picked;
                self.status = said.to_owned();
                true
            }
            Err(error) => {
                if let Some(document) = &mut self.document {
                    document.undo();
                    let text = document.text();
                    let _ = self.reload_text(&text, true);
                }
                self.status = error;
                false
            }
        }
    }

    /// Name the scene at `index` what was typed, and reload; a name
    /// another scene has is refused.
    pub(super) fn rename_scene(&mut self, index: usize, typed: &str) -> Task<Message> {
        let Some(document) = &mut self.document else {
            return Task::none();
        };
        if let Err(error) = scenes::rename(document, index, typed) {
            self.status = format!("cannot rename: {error}");
            return Task::none();
        }
        let text = document.text();
        match self.reload_text(&text, true) {
            Ok(()) => {
                if let Some((_, typed)) = &mut self.field_typed {
                    typed.retain(|(label, _)| *label != "name");
                }
                self.status = format!("scene named {}", typed.trim());
            }
            Err(error) => {
                if let Some(document) = &mut self.document {
                    document.undo();
                }
                self.status = error;
            }
        }
        Task::none()
    }

    pub(super) fn type_trigger(&mut self, row: Option<usize>, typed: String) {
        self.triggers_typed.insert(row, typed);
    }

    /// Write every trigger row typed into but `keep` into the picked
    /// scene, as one step, and reload.
    pub(super) fn apply_triggers(&mut self, keep: Option<&editing::Typed>) -> Task<Message> {
        let (Some(index), Some(document)) = (self.scene, &self.document) else {
            return Task::none();
        };
        let applied: Vec<Option<usize>> = self
            .triggers_typed
            .keys()
            .copied()
            .filter(|row| keep != Some(&editing::Typed::Trigger(*row)))
            .collect();
        if applied.is_empty() {
            return Task::none();
        }
        let mut names = scenes::triggers(document, index);
        for (i, name) in names.iter_mut().enumerate() {
            if applied.contains(&Some(i))
                && let Some(typed) = self.triggers_typed.get(&Some(i))
            {
                typed.clone_into(name);
            }
        }
        if applied.contains(&None)
            && let Some(typed) = self.triggers_typed.get(&None)
        {
            names.push(typed.clone());
        }
        self.write_triggers(index, &names, &applied)
    }

    /// Take a trigger out of the picked scene.
    pub(super) fn remove_trigger(&mut self, row: usize) -> Task<Message> {
        let (Some(index), Some(document)) = (self.scene, &self.document) else {
            return Task::none();
        };
        let mut names = scenes::triggers(document, index);
        if row >= names.len() {
            return Task::none();
        }
        names.remove(row);
        self.triggers_typed.clear();
        self.write_triggers(index, &names, &[])
    }

    /// Set the scene's triggers to `names` and reload; the rows in
    /// `applied` are no longer typed into once it loads.
    fn write_triggers(
        &mut self,
        index: usize,
        names: &[String],
        applied: &[Option<usize>],
    ) -> Task<Message> {
        let Some(document) = &mut self.document else {
            return Task::none();
        };
        let revision = document.revision();
        if let Err(error) = scenes::set_triggers(document, index, names) {
            self.status = format!("could not set the triggers: {error}");
            return Task::none();
        }
        let changed = document.revision() != revision;
        let text = document.text();
        let reloaded = if changed {
            self.reload_text(&text, true)
        } else {
            Ok(())
        };
        match reloaded {
            Ok(()) => {
                self.triggers_typed.retain(|row, _| !applied.contains(row));
                if changed {
                    let now = self
                        .document
                        .as_ref()
                        .map(|d| scenes::triggers(d, index))
                        .unwrap_or_default();
                    self.status = if now.is_empty() {
                        "the scene has no trigger now".to_owned()
                    } else {
                        format!("entered by {}", now.join(", "))
                    };
                }
            }
            Err(error) => {
                if let Some(document) = &mut self.document {
                    document.undo();
                }
                self.status = error;
            }
        }
        Task::none()
    }

    /// The picked scene in the inspector: its name, the triggers that
    /// enter it, the output it sets over the show's, what entering it
    /// does, and when this session entered it and from where.
    pub(super) fn scene_panel<'a>(
        &'a self,
        session: &'a Session,
        index: usize,
    ) -> Column<Element<'a, Message>> {
        let mut panel = Column::new().spacing(4).padding(12);
        let Some(document) = &self.document else {
            return panel;
        };
        let names = scenes::names(document);
        let Some(written) = names.get(index) else {
            return panel.push(text("the picked scene is gone").size(14).boxed());
        };
        let heading = |label: &'a str| container(text(label).size(12)).padding([6, 0]).boxed();
        let name = self.field_text("name").unwrap_or(written);
        panel = panel.push(text(name.to_owned()).size(16).boxed());
        let active = session.active_scene().as_deref() == Some(written.as_str());
        let mut said = format!("scene {} of {}", index + 1, names.len());
        if index == 0 {
            said.push_str(", the one the show starts in");
        }
        if active {
            said.push_str(", active");
        }
        panel = panel.push(text(said).size(12).boxed());

        panel = panel.push(heading("SCENE"));
        for field in &self.layer_fields {
            if field.field.path == ["name"] {
                panel = panel.push(self.field_row(field));
            }
        }

        panel = panel.push(heading("TRIGGERS"));
        let triggers = scenes::triggers(document, index);
        if triggers.is_empty() && index > 0 {
            panel = panel.push(
                text("No trigger enters it: only the first scene is entered without one.")
                    .size(12)
                    .boxed(),
            );
        }
        for (i, trigger) in triggers.into_iter().enumerate() {
            panel = panel.push(self.trigger_row(Some(i), trigger));
        }
        panel = panel.push(self.trigger_row(None, String::new()));

        panel = panel.push(heading("OUTPUT"));
        panel = panel.push(
            text(
                "Set over the show's while the scene is active; what it leaves out is the show's.",
            )
            .size(12)
            .boxed(),
        );
        for field in &self.layer_fields {
            if field.field.path.first() == Some(&"output") {
                panel = panel.push(self.field_row(field));
            }
        }

        panel = panel.push(heading("ENTERING IT"));
        let entering = {
            let engine = lock(&session.engine);
            engine
                .show()
                .map(|show| scenes::entering(show, index))
                .unwrap_or_default()
        };
        let lines = [
            ("restarts from 0", &entering.restarts),
            ("starts again where its while holds", &entering.whiles),
            (
                "plays only if its when turned true while the scene was away",
                &entering.whens,
            ),
            (
                "also starts, on the trigger that entered it",
                &entering.triggered,
            ),
        ];
        for (what, timelines) in lines {
            if !timelines.is_empty() {
                panel = panel.push(
                    Column::with_children([
                        text(what).size(12).boxed(),
                        text(timelines.join("\n")).size(13).boxed(),
                    ])
                    .spacing(2)
                    .padding([2, 0])
                    .boxed(),
                );
            }
        }
        panel = panel.push(
            text("Stops the timelines of the scene it leaves and drops what they held; variables keep their values.")
                .size(12)
                .boxed(),
        );

        panel = panel.push(heading("ENTERED"));
        let entries: Vec<_> = session
            .entries
            .iter()
            .filter(|e| e.scene == *written)
            .collect();
        if entries.is_empty() {
            panel = panel.push(text("not yet in this session").size(12).boxed());
        }
        for entry in entries.iter().rev().take(ENTRIES_SHOWN) {
            let from = match &entry.from {
                Some(from) => format!(" from {from}"),
                None => String::new(),
            };
            panel = panel.push(
                text(format!("{:7.2} s{from}, {}", entry.at, entry.by))
                    .size(12)
                    .boxed(),
            );
        }
        panel
    }

    /// One trigger of the picked scene, typed into until Enter or until
    /// it is left; `None` is the row that adds one.
    fn trigger_row<'a>(&'a self, row: Option<usize>, written: String) -> Element<'a, Message> {
        let typed = self.triggers_typed.get(&row);
        let shown = typed.cloned().unwrap_or(written);
        let marked = super::inspector::field_style(typed.is_some(), false);
        row![
            text_input("add a trigger", shown)
                .on_input(move |typed| Message::TypeTrigger(row, typed))
                .on_submit(Message::ApplyTriggers)
                .size(13)
                .padding([1, 4])
                .width(Fill)
                .style(marked),
            super::inspector::reset(row.map(Message::RemoveTrigger)),
        ]
        .spacing(6)
        .align_y(iced::Center)
        .boxed()
    }
}
