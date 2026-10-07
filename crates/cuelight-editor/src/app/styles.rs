//! The show's font styles: listed in the assets under the font file each
//! draws with, and edited in the inspector like a layer's fields.

use cuelight_core::LayerKind;
use cuelight_editor_core::session::{Session, lock};
use cuelight_editor_core::tree::{self, Row};
use iced::widget::Widget as _;
use iced::widget::{Column, button, container, row, space, text, text_input};
use iced::{Element, Fill, Task};
use serde_json::Value;

use cuelight_editor_core::document::{Part, Pointer};

use super::editing::style_pointer;
use super::{App, Message, inspector};

/// The picked style's name field: an id of its own keeps its focus
/// while the panel around it is laid out again.
const STYLE_NAME: &str = "style-name";

impl App {
    /// The styles that draw with the font file `font`, each a row to
    /// pick, and a row that adds one.
    pub(super) fn style_rows<'a>(&'a self, font: &'a str) -> Column<Element<'a, Message>> {
        let mut rows = Column::new().spacing(2);
        for name in self.styles_of(font) {
            let line = row![
                space::horizontal().width(48),
                text("style").size(11).width(44),
                text(name.clone()).size(13),
            ]
            .spacing(6)
            .align_y(iced::Center);
            let mut b = button(line)
                .on_press(Message::PickStyle(name.clone()))
                .width(Fill)
                .padding([1, 6])
                .style(button::text);
            if self.style.as_deref() == Some(name.as_str()) {
                b = b.style(button::secondary);
            }
            rows = rows.push(b.boxed());
        }
        rows.push(
            row![
                space::horizontal().width(48),
                button(text("+ style").size(12))
                    .on_press(Message::AddStyle(font.to_owned()))
                    .padding([1, 6])
                    .style(button::text),
            ]
            .boxed(),
        )
    }

    /// The names of the styles drawing with `font`, as the document has
    /// them.
    fn styles_of(&self, font: &str) -> Vec<String> {
        let fonts = self
            .document
            .as_ref()
            .and_then(|d| {
                d.get(&cuelight_editor_core::document::Pointer::default().then(
                    cuelight_editor_core::document::Part::Key("fonts".to_owned()),
                ))
            })
            .map(|n| n.value());
        let Some(Value::Object(styles)) = fonts else {
            return Vec::new();
        };
        styles
            .into_iter()
            .filter(|(_, style)| style.get("file").and_then(Value::as_str) == Some(font))
            .map(|(name, _)| name)
            .collect()
    }

    /// A picked font style: its fields, the text layers that use it, each
    /// a link to the layer, and its JSON.
    pub(super) fn style_panel<'a>(
        &'a self,
        session: &'a Session,
        name: &'a str,
    ) -> Column<Element<'a, Message>> {
        let mut panel = Column::new().spacing(4).padding(12);
        let Some(document) = &self.document else {
            return panel;
        };
        let Some(json) = document.text_at(&style_pointer(name)) else {
            return panel.push(text("the picked style is gone").size(14).boxed());
        };
        panel = panel.push(text(name.to_owned()).size(16).boxed());
        panel = panel.push(text("font style").size(12).boxed());
        panel = panel.push(container(text("STYLE").size(12)).padding([6, 0]).boxed());
        // Its name, which the text layers using it follow when it changes.
        let typed = self.style_name_typed.clone();
        panel = panel.push(
            row![
                container(text("name").size(13)).width(86).padding([2, 6]),
                text_input(name, typed.clone().unwrap_or_else(|| name.to_owned()))
                    .id(STYLE_NAME)
                    .on_input(Message::TypeStyleName)
                    .on_submit(Message::ApplyStyleName)
                    .size(13)
                    .padding([1, 4])
                    .width(Fill)
                    .style(inspector::field_style(typed.is_some(), false)),
            ]
            .spacing(6)
            .align_y(iced::Center)
            .boxed(),
        );
        for field in &self.layer_fields {
            panel = panel.push(self.field_row(field));
        }

        panel = panel.push(container(text("USED BY").size(12)).padding([6, 0]).boxed());
        let engine = lock(&session.engine);
        let users: Vec<(cuelight_core::LayerPath, String)> = engine
            .show()
            .map(|show| {
                self.rows
                    .iter()
                    .filter_map(|row| match row {
                        Row::Layer { path, .. } => Some(path),
                        _ => None,
                    })
                    .filter(|path| {
                        tree::layer(show, path).is_some_and(|layer| {
                            matches!(&layer.kind, LayerKind::Text { font, .. } if font == name)
                        })
                    })
                    .map(|path| (path.clone(), tree::describe(show, path)))
                    .collect()
            })
            .unwrap_or_default();
        drop(engine);
        if users.is_empty() {
            // Nothing draws in it: it can go.
            panel = panel.push(
                row![
                    text("no text layer in this show").size(13).width(Fill),
                    button(text("Remove style").size(12))
                        .on_press(Message::RemoveStyle)
                        .style(button::danger),
                ]
                .align_y(iced::Center)
                .boxed(),
            );
        }
        for (path, place) in users {
            panel = panel.push(
                button(text(place).size(13))
                    .on_press(Message::Choose(path))
                    .width(Fill)
                    .padding([2, 6])
                    .style(button::text)
                    .boxed(),
            );
        }

        panel = panel.push(container(text("JSON").size(12)).padding([6, 0]).boxed());
        panel.push(inspector::json_text(&json, &super::theme(self)))
    }

    /// Add a style drawing with the font file `font`, named after it, and
    /// pick it: an outline font gets a size, which it needs.
    pub(super) fn add_style(&mut self, font: String) -> Task<Message> {
        let outline = self.session.as_ref().is_some_and(|s| {
            lock(&s.engine)
                .outline_fonts()
                .any(|(name, _)| name == font)
        });
        let taken = |name: &str| {
            self.document
                .as_ref()
                .is_some_and(|d| d.get(&style_pointer(name)).is_some())
        };
        let name = (1..)
            .map(|n| {
                if n == 1 {
                    font.clone()
                } else {
                    format!("{font}_{n}")
                }
            })
            .find(|name| !taken(name))
            .unwrap_or_else(|| font.clone());
        let style = if outline {
            serde_json::json!({"file": font, "size": 16})
        } else {
            serde_json::json!({"file": font})
        };
        let Some(document) = &mut self.document else {
            return Task::none();
        };
        let fonts = cuelight_editor_core::document::Pointer::default().then(
            cuelight_editor_core::document::Part::Key("fonts".to_owned()),
        );
        let done = if document.get(&fonts).is_some() {
            document.insert(&style_pointer(&name), style)
        } else {
            let mut all = serde_json::Map::new();
            all.insert(name.clone(), style);
            document.insert(&fonts, Value::Object(all))
        };
        if let Err(error) = done {
            self.status = format!("could not add a style: {error}");
            return Task::none();
        }
        let text = document.text();
        match self.reload_text(&text, true) {
            Ok(()) => {
                self.status = format!("added the font style {name}");
                self.selected = None;
                self.style = Some(name);
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

    /// Give the picked style the name typed for it, and every text layer
    /// that uses it the new name, as one step. A name another style has
    /// is refused.
    pub(super) fn rename_style(&mut self) -> Task<Message> {
        let (Some(from), Some(typed)) = (self.style.clone(), self.style_name_typed.take()) else {
            return Task::none();
        };
        let to = typed.trim().to_owned();
        if to.is_empty() || to == from {
            return Task::none();
        }
        // Where each text layer drawing in it writes its font.
        let users: Vec<Pointer> = self
            .session
            .as_ref()
            .map(|session| {
                let engine = lock(&session.engine);
                let Some(show) = engine.show() else {
                    return Vec::new();
                };
                self.rows
                    .iter()
                    .filter_map(|row| match row {
                        Row::Layer { path, .. } => Some(path),
                        _ => None,
                    })
                    .filter(|path| {
                        tree::layer(show, path).is_some_and(|layer| {
                            matches!(&layer.kind, LayerKind::Text { font, .. } if *font == from)
                        })
                    })
                    .filter_map(|path| Pointer::parse(&tree::pointer(show, path)?).ok())
                    .map(|at| at.then(Part::Key("font".to_owned())))
                    .collect()
            })
            .unwrap_or_default();
        let Some(document) = &mut self.document else {
            return Task::none();
        };
        if document.get(&style_pointer(&to)).is_some() {
            self.status = format!("there is a font style {to} already");
            self.style_name_typed = Some(typed);
            return Task::none();
        }
        let before = document.text();
        document.begin_step();
        let mut done = document.rename(&style_pointer(&from), &to);
        for at in &users {
            if done.is_ok() && document.get(at).is_some() {
                done = document.set(at, Value::from(to.clone()));
            }
        }
        document.end_step();
        if let Err(error) = done {
            // Only what this rename wrote is taken back.
            if document.text() != before {
                document.undo();
            }
            self.status = format!("could not rename {from}: {error}");
            self.style_name_typed = Some(typed);
            return Task::none();
        }
        let text = document.text();
        match self.reload_text(&text, true) {
            Ok(()) => {
                self.style = Some(to.clone());
                self.status = format!(
                    "renamed the font style {from} to {to}, and {} text layer(s)",
                    users.len()
                );
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

    /// Take the picked style out of the show, when no text layer draws
    /// in it; a show left with no styles loses its `fonts` too.
    pub(super) fn remove_style(&mut self) -> Task<Message> {
        let Some(name) = self.style.clone() else {
            return Task::none();
        };
        let used = self.session.as_ref().is_some_and(|session| {
            let engine = lock(&session.engine);
            engine.show().is_some_and(|show| {
                self.rows.iter().any(|row| {
                    match row {
                    Row::Layer { path, .. } => tree::layer(show, path).is_some_and(|layer| {
                        matches!(&layer.kind, LayerKind::Text { font, .. } if *font == name)
                    }),
                    _ => false,
                }
                })
            })
        });
        if used {
            self.status = format!("the font style {name} is in use");
            return Task::none();
        }
        let Some(document) = &mut self.document else {
            return Task::none();
        };
        if let Err(error) = cuelight_editor_core::fields::remove_path(
            document,
            &Pointer::default(),
            &["fonts", &name],
        ) {
            self.status = format!("could not remove {name}: {error}");
            return Task::none();
        }
        let text = document.text();
        match self.reload_text(&text, true) {
            Ok(()) => {
                self.style = None;
                self.style_name_typed = None;
                self.status = format!("removed the font style {name}");
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
}
