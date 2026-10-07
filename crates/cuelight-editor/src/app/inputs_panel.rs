//! The inputs panel: the show's triggers, variables and values, each
//! with a button to rename it.

use std::collections::BTreeMap;

use cuelight_editor_core::inputs::{self, Place};
use cuelight_editor_core::renames::Kind;
use iced::keyboard;
use iced::widget::Widget as _;
use iced::widget::{Column, button, row, text, text_input, toggler};
use iced::{Element, Fill};

use super::{App, Message};
use cuelight_editor_core::session::Session;

impl App {
    /// The show's inputs: triggers as buttons, variables as fields, the
    /// show's own values as readouts.
    pub(super) fn inputs_panel<'a>(&'a self, session: &'a Session) -> Column<Element<'a, Message>> {
        let mut panel = Column::new().spacing(6).padding(12);
        panel = panel.push(
            toggler(session.recording)
                .label("Record what I fire")
                .on_toggle(Message::Record)
                .size(16)
                .boxed(),
        );
        // Triggers by where they are heard: the ones that open a scene,
        // the ones heard anywhere, then each scene's own, dimmed while
        // another scene is up.
        let active = session.active_scene();
        let mut opens = Vec::new();
        let mut anywhere = Vec::new();
        let mut by_scene: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for trigger in &self.inputs.triggers {
            match self.inputs.places.get(trigger) {
                Some(Place::Opens(_)) => opens.push(trigger.as_str()),
                Some(Place::Scene(scene)) => {
                    by_scene.entry(scene.as_str()).or_default().push(trigger)
                }
                _ => anywhere.push(trigger.as_str()),
            }
        }
        if !opens.is_empty() {
            panel = self.triggers(panel, "OPENS A SCENE".to_owned(), &opens, true);
        }
        if !anywhere.is_empty() {
            panel = self.triggers(panel, "ANYWHERE".to_owned(), &anywhere, true);
        }
        for (scene, triggers) in &by_scene {
            let live = active.as_deref() == Some(*scene);
            panel = self.triggers(panel, format!("IN {scene}"), triggers, live);
        }
        if !self.inputs.variables.is_empty() {
            panel = panel.push(text("VARIABLES").size(12).boxed());
            for (name, initial) in &self.inputs.variables {
                if let Some(field) = self.rename_field(Kind::Variable, name) {
                    panel = panel.push(field);
                    continue;
                }
                let current = session.value(name).unwrap_or_else(|| initial.clone());
                let control: Element<'a, Message> = match current {
                    cuelight_core::Value::Bool(on) => toggler(on)
                        .on_toggle(move |on| Message::Set(name.clone(), on.to_string()))
                        .size(16)
                        .boxed(),
                    _ => {
                        let shown: &'a str =
                            self.fields.get(name).map(String::as_str).unwrap_or("");
                        text_input("", shown)
                            .on_input(move |t| Message::Edit(name.clone(), t))
                            .on_submit(Message::Set(name.clone(), shown.to_owned()))
                            .size(14)
                            .width(110)
                            .boxed()
                    }
                };
                panel = panel.push(
                    row![
                        text(name).size(14).width(Fill),
                        control,
                        App::rename_button(Kind::Variable, name),
                    ]
                    .spacing(8)
                    .align_y(iced::Center)
                    .boxed(),
                );
            }
        }
        if !self.inputs.values.is_empty() {
            panel = panel.push(text("VALUES").size(12).boxed());
            for name in &self.inputs.values {
                if let Some(field) = self.rename_field(Kind::Value, name) {
                    panel = panel.push(field);
                    continue;
                }
                let shown = session
                    .value(name)
                    .map(|v| inputs::show_value(&v))
                    .unwrap_or_default();
                panel = panel.push(
                    row![
                        text(name).size(14).width(Fill),
                        text(shown).size(14),
                        App::rename_button(Kind::Value, name),
                    ]
                    .spacing(8)
                    .align_y(iced::Center)
                    .boxed(),
                );
            }
        }
        panel
    }
}

impl App {
    /// One heading of trigger buttons. A button names the keys that fire
    /// its trigger; a section whose scene is not up is drawn dimmed.
    fn triggers<'a>(
        &'a self,
        panel: Column<Element<'a, Message>>,
        heading: String,
        triggers: &[&'a str],
        live: bool,
    ) -> Column<Element<'a, Message>> {
        let mut panel = panel.push(text(heading).size(12).boxed());
        for trigger in triggers {
            if let Some(field) = self.rename_field(Kind::Trigger, trigger) {
                panel = panel.push(field);
                continue;
            }
            let keys: Vec<&str> = self
                .inputs
                .keys
                .iter()
                .filter(|(_, t)| t == trigger)
                .map(|(k, _)| if k == " " { "Space" } else { k.as_str() })
                .collect();
            let label = if keys.is_empty() {
                (*trigger).to_owned()
            } else {
                format!("{trigger}  [{}]", keys.join(", "))
            };
            let mut b = button(text(label).size(14))
                .on_press(Message::Fire((*trigger).to_owned()))
                .width(Fill);
            if !live {
                b = b.style(button::secondary);
            }
            panel = panel.push(
                row![b, App::rename_button(Kind::Trigger, trigger)]
                    .spacing(6)
                    .align_y(iced::Center)
                    .boxed(),
            );
        }
        panel
    }
}

/// A key's name as a browser gives it, which is how a show names one.
pub(super) fn key_name(key: &keyboard::Key) -> String {
    match key {
        keyboard::Key::Character(c) => c.to_string(),
        keyboard::Key::Named(keyboard::key::Named::Space) => " ".to_owned(),
        keyboard::Key::Named(named) => format!("{named:?}"),
        keyboard::Key::Unidentified => String::new(),
    }
}
