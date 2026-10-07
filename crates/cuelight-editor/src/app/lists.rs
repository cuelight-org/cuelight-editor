//! The show's lists in its inspector, row by row: the keys that fire
//! triggers and the variables it declares. What is typed into a row is
//! held until Enter or until the row is left, like a field.

use cuelight_editor_core::lists::List;
use iced::widget::Widget as _;
use iced::widget::{Column, container, row, text, text_input};
use iced::{Element, Fill, Task};

use super::{App, Message, editing};

/// A row typed into, by its list and the name it had (`None` for the
/// row that adds one).
pub(super) type Row = (List, Option<String>);

/// What was typed into a row: its name, its value, or both.
#[derive(Debug, Clone, Default)]
pub(super) struct Draft {
    pub name: Option<String>,
    pub value: Option<String>,
}

impl App {
    /// A list's heading, its rows and an empty row that adds one.
    pub(super) fn list_panel<'a>(&'a self, list: List) -> Element<'a, Message> {
        let Some(document) = &self.document else {
            return Column::<Element<'a, Message>>::new().boxed();
        };
        let mut panel = Column::<Element<'a, Message>>::new().spacing(4);
        panel = panel.push(
            container(text(list.heading()).size(12))
                .padding([6, 0])
                .boxed(),
        );
        for (name, value) in list.rows(document) {
            panel = panel.push(self.list_row(list, Some(name), value));
        }
        panel = panel.push(self.list_row(list, None, String::new()));
        panel.boxed()
    }

    fn list_row<'a>(
        &'a self,
        list: List,
        was: Option<String>,
        value: String,
    ) -> Element<'a, Message> {
        let (name_hint, value_hint) = list.hints();
        let draft = self.list_typed.get(&(list, was.clone()));
        let shown_name = draft
            .and_then(|d| d.name.clone())
            .or_else(|| was.clone())
            .unwrap_or_default();
        let shown_value = draft.and_then(|d| d.value.clone()).unwrap_or(value);
        let marked = super::inspector::field_style(draft.is_some(), false);
        let on_name = {
            let was = was.clone();
            move |typed| Message::TypeListName(list, was.clone(), typed)
        };
        let on_value = {
            let was = was.clone();
            move |typed| Message::TypeListValue(list, was.clone(), typed)
        };
        let remove = was.clone().map(|name| Message::RemoveListRow(list, name));
        row![
            text_input(name_hint, shown_name)
                .on_input(on_name)
                .on_submit(Message::ApplyList(list, was.clone()))
                .size(13)
                .padding([1, 4])
                .width(Fill)
                .style(marked),
            text_input(value_hint, shown_value)
                .on_input(on_value)
                .on_submit(Message::ApplyList(list, was))
                .size(13)
                .padding([1, 4])
                .width(Fill)
                .style(marked),
            super::inspector::reset(remove),
        ]
        .spacing(6)
        .align_y(iced::Center)
        .boxed()
    }

    pub(super) fn type_list(&mut self, at: Row, name: Option<String>, value: Option<String>) {
        let draft = self.list_typed.entry(at).or_default();
        if name.is_some() {
            draft.name = name;
        }
        if value.is_some() {
            draft.value = value;
        }
    }

    /// Enter in a row, or leaving it: write it into the show and reload.
    /// A new row waits until it has a name and a value.
    pub(super) fn apply_list(&mut self, at: Row) -> Task<Message> {
        let Some(draft) = self.list_typed.get(&at).cloned() else {
            return Task::none();
        };
        let (list, was) = at.clone();
        let Some(document) = &self.document else {
            return Task::none();
        };
        let current = was.as_ref().and_then(|was| {
            list.rows(document)
                .into_iter()
                .find(|(name, _)| name == was)
                .map(|(_, value)| value)
        });
        let Some(name) = draft.name.clone().or_else(|| was.clone()) else {
            return Task::none();
        };
        let Some(typed) = draft.value.clone().or(current) else {
            return Task::none();
        };
        let value = match list.parse(&typed) {
            Ok(value) => value,
            Err(error) => {
                self.status = error;
                return Task::none();
            }
        };
        let Some(document) = &mut self.document else {
            return Task::none();
        };
        if let Err(error) = list.put(document, was.as_deref(), &name, value) {
            self.status = error;
            return Task::none();
        }
        let text = document.text();
        match self.reload_text(&text, true) {
            Ok(()) => {
                self.list_typed.remove(&at);
                self.status = format!(
                    "{} {} = {typed}",
                    list.heading().to_lowercase(),
                    name.trim()
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

    pub(super) fn remove_list_row(&mut self, list: List, name: String) -> Task<Message> {
        let Some(document) = &mut self.document else {
            return Task::none();
        };
        if let Err(error) = list.remove(document, &name) {
            self.status = format!("could not remove: {error}");
            return Task::none();
        }
        let text = document.text();
        match self.reload_text(&text, true) {
            Ok(()) => {
                self.list_typed.remove(&(list, Some(name.clone())));
                self.status = format!("{name} removed");
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

    /// Apply every row typed into but `keep`, the way leaving a field
    /// applies it; a new row without both its name and value waits.
    pub(super) fn commit_lists(&mut self, keep: Option<&editing::Typed>) {
        let rows: Vec<Row> = self.list_typed.keys().cloned().collect();
        for at in rows {
            if keep == Some(&editing::Typed::Row(at.0, at.1.clone())) {
                continue;
            }
            let incomplete = at.1.is_none()
                && self
                    .list_typed
                    .get(&at)
                    .is_none_or(|d| d.name.is_none() || d.value.is_none());
            if !incomplete {
                let _ = self.apply_list(at);
            }
        }
    }
}
