//! Renaming a trigger, a variable or a value: a new name typed where it
//! is listed, then every place it changes listed in a bar for a yes,
//! then made as one step to undo.

use cuelight_editor_core::renames::{self, Kind, Plan};
use iced::widget::Widget as _;
use iced::widget::{Column, button, column, container, row, scrollable, text, text_input};
use iced::{Element, Fill, Task};

use super::{App, Message};

/// A name being typed in place of another, before it is asked about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Renaming {
    pub kind: Kind,
    pub from: String,
    pub typed: String,
}

impl App {
    /// The field that takes a new name, in place of the name's row.
    pub(super) fn rename_field<'a>(
        &'a self,
        kind: Kind,
        name: &str,
    ) -> Option<Element<'a, Message>> {
        let renaming = self
            .renaming
            .as_ref()
            .filter(|r| r.kind == kind && r.from == name)?;
        Some(
            row![
                text_input("new name", &renaming.typed)
                    .on_input(Message::TypeRename)
                    .on_submit(Message::ProposeRename)
                    .size(14)
                    .width(Fill),
                button(text("Cancel").size(12))
                    .on_press(Message::CancelRename)
                    .style(button::secondary),
            ]
            .spacing(6)
            .align_y(iced::Center)
            .boxed(),
        )
    }

    /// The small button beside a name that starts renaming it.
    pub(super) fn rename_button<'a>(kind: Kind, name: &str) -> Element<'a, Message> {
        button(text("Rename").size(12))
            .on_press(Message::StartRename(kind, name.to_owned()))
            .style(button::secondary)
            .padding([2, 6])
            .boxed()
    }

    pub(super) fn start_rename(&mut self, kind: Kind, from: String) {
        self.rename = None;
        self.renaming = Some(Renaming {
            kind,
            typed: from.clone(),
            from,
        });
    }

    /// Enter in the field: work out what the rename changes, and ask.
    pub(super) fn propose_typed_rename(&mut self) {
        if let Some(Renaming { kind, from, typed }) = self.renaming.clone() {
            self.propose_rename(kind, &from, typed.trim());
        }
    }

    /// Work out renaming `from` to `to` and put what it changes up for a
    /// yes; a name already in use is refused in the status line.
    pub(super) fn propose_rename(&mut self, kind: Kind, from: &str, to: &str) {
        let Some(document) = &self.document else {
            return;
        };
        let driver = renames::driver_text(document, &self.files)
            .and_then(|text| serde_json::from_str(&text).ok());
        match renames::plan(kind, from, to, &document.value(), driver.as_ref()) {
            Ok(plan) => {
                self.status = format!(
                    "renaming {} {from} to {to} changes {} place(s)",
                    kind.word(),
                    plan.uses.len()
                );
                self.rename = Some(plan);
            }
            Err(error) => {
                self.rename = None;
                self.status = format!("cannot rename {from}: {error}");
            }
        }
    }

    /// Make the rename asked about, as one step to undo.
    pub(super) fn apply_rename(&mut self) -> Task<Message> {
        let Some(asked) = self.rename.take() else {
            return Task::none();
        };
        self.renaming = None;
        let Some(document) = &mut self.document else {
            return Task::none();
        };
        // Worked out again on the show as it is now, in case it moved on
        // while the question was up.
        let driver = renames::driver_text(document, &self.files);
        let parsed = driver.as_deref().and_then(|t| serde_json::from_str(t).ok());
        let plan = renames::plan(
            asked.kind,
            &asked.from,
            &asked.to,
            &document.value(),
            parsed.as_ref(),
        );
        let done =
            plan.and_then(|plan| renames::apply(&plan, document, driver.as_deref()).map(|()| plan));
        let plan = match done {
            Ok(plan) => plan,
            Err(error) => {
                self.status = format!("could not rename {}: {error}", asked.from);
                return Task::none();
            }
        };
        let text = document.text();
        self.follow_driver();
        match self.reload_text(&text, true) {
            Ok(()) => {
                self.edits.remove(&plan.from);
                self.list_typed
                    .retain(|(_, was), _| was.as_deref() != Some(plan.from.as_str()));
                self.status = format!(
                    "renamed {} {} to {} in {} place(s)",
                    plan.kind.word(),
                    plan.from,
                    plan.to,
                    plan.uses.len()
                );
            }
            Err(error) => {
                if let Some(document) = &mut self.document {
                    document.undo();
                }
                self.follow_driver();
                self.status = error;
            }
        }
        Task::none()
    }

    pub(super) fn cancel_rename(&mut self) {
        self.rename = None;
        self.renaming = None;
    }

    /// Play by the driver as the document's edits left it, after a
    /// rename or its undo.
    pub(super) fn follow_driver(&mut self) {
        let Some(document) = &self.document else {
            return;
        };
        if let (Some(driver), Some(session)) = (renames::edited_driver(document), &mut self.session)
        {
            session.set_driver(Some(driver));
        }
    }

    /// The rename waiting for a yes: what it is, then every place it
    /// changes.
    pub(super) fn rename_prompt(&self) -> Option<Element<'_, Message>> {
        let plan = self.rename.as_ref()?;
        let Plan {
            kind,
            from,
            to,
            uses,
        } = plan;
        let ask = super::question(
            format!(
                "Rename {} {from} to {to}? This changes {} place(s):",
                kind.word(),
                uses.len()
            ),
            [
                ("Rename", Message::ApplyRename),
                ("Cancel", Message::CancelRename),
            ],
        );
        let height = (uses.len() as f32 * 20.0).min(180.0);
        let mut list = Column::<Element<'_, Message>>::new().spacing(2);
        for found in uses {
            let file = if found.file == renames::DRIVER {
                format!("{}  {}", renames::DRIVER, found.at)
            } else {
                found.at.to_string()
            };
            list = list.push(
                row![text(&found.what).size(13).width(Fill), text(file).size(11),]
                    .spacing(12)
                    .boxed(),
            );
        }
        Some(
            column![
                ask,
                // A long list scrolls, so the stage keeps its room.
                container(scrollable(list.padding([0, 14])).height(height)).padding([0, 12]),
            ]
            .boxed(),
        )
    }
}
