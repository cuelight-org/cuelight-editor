//! Editing the picked layer from the inspector: what is typed into a
//! property's field goes into the document, the show reloads at the
//! playhead, and Ctrl+Z takes it back.

use super::*;
use cuelight_core::Influence;
use cuelight_editor_core::document::Pointer;
use cuelight_editor_core::edit;

/// An edit held back because something other than the base owns the
/// property at the playhead: a new base would not show until it lets go.
#[derive(Debug, Clone)]
pub(super) struct Owned {
    pub path: LayerPath,
    pub property: Property,
    pub value: serde_json::Value,
    /// What owns it, in words: `timeline enter`, `binding on speed`.
    pub owner: String,
}

impl App {
    /// What is in a property's field now: what was typed into it, if the
    /// picked layer is still the one it was typed for.
    pub(super) fn typed(&self, path: &LayerPath, property: Property) -> Option<&str> {
        let (typed_for, fields) = self.typed.as_ref()?;
        (typed_for == path)
            .then(|| {
                fields
                    .iter()
                    .find(|(p, _)| *p == property)
                    .map(|(_, t)| t.as_str())
            })
            .flatten()
    }

    /// What each editable property's field shows: what was typed, or
    /// the base value.
    pub(super) fn field(&self, path: &LayerPath, property: Property) -> Option<&str> {
        self.typed(path, property).or_else(|| {
            self.bases
                .iter()
                .find(|(p, _)| *p == property)
                .map(|(_, base)| base.as_str())
        })
    }

    /// The base values of the picked layer's editable properties, after
    /// whatever just changed.
    pub(super) fn refresh_bases(&mut self) {
        self.bases.clear();
        let (Some(session), Some(path)) = (&self.session, self.selection.last()) else {
            return;
        };
        let engine = lock(&session.engine);
        for property in tree::PROPERTIES {
            if edit::input(property).is_none() {
                continue;
            }
            if let Some(base) =
                engine
                    .explain(path, property)
                    .iter()
                    .find_map(|source| match source {
                        Influence::Base { value } => Some(inputs::show_value(value)),
                        _ => None,
                    })
            {
                self.bases.push((property, base));
            }
        }
    }

    pub(super) fn type_into(&mut self, property: Property, text: String) {
        let Some(path) = self.selection.last().cloned() else {
            return;
        };
        match &mut self.typed {
            Some((typed_for, fields)) if *typed_for == path => {
                fields.retain(|(p, _)| *p != property);
                fields.push((property, text));
            }
            _ => self.typed = Some((path, vec![(property, text)])),
        }
    }

    /// Enter in a property's field: set the base value to what was typed,
    /// or ask first when a timeline or binding owns the property now.
    pub(super) fn apply(&mut self, property: Property) -> Task<Message> {
        let Some(path) = self.selection.last().cloned() else {
            return Task::none();
        };
        let Some(typed) = self.typed(&path, property).map(str::to_owned) else {
            return Task::none();
        };
        let value = match edit::parse(property, &typed) {
            Ok(value) => value,
            Err(error) => {
                self.status = error;
                return Task::none();
            }
        };
        let owner = self.session.as_ref().and_then(|session| {
            let engine = lock(&session.engine);
            match engine.explain(&path, property).first() {
                Some(Influence::Timeline { timeline, .. }) => {
                    Some(format!("timeline {}", timeline.name))
                }
                Some(Influence::Binding { variable, .. }) => {
                    Some(format!("a binding on {variable}"))
                }
                _ => None,
            }
        });
        if let Some(owner) = owner {
            self.owned = Some(Owned {
                path,
                property,
                value,
                owner,
            });
            return Task::none();
        }
        self.edit_base(&path, property, value);
        Task::none()
    }

    /// Set the base value anyway, after the question.
    pub(super) fn edit_owned(&mut self) -> Task<Message> {
        if let Some(Owned {
            path,
            property,
            value,
            ..
        }) = self.owned.take()
        {
            self.edit_base(&path, property, value);
        }
        Task::none()
    }

    /// Write `value` as the base of `property` on the layer at `path`
    /// and reload at the playhead; a show that no longer loads takes the
    /// edit back.
    fn edit_base(&mut self, path: &LayerPath, property: Property, value: serde_json::Value) {
        let Some(at) = self.layer_pointer(path) else {
            self.status = "the document does not have this layer where the show does".to_owned();
            return;
        };
        let Some(document) = &mut self.document else {
            return;
        };
        if let Err(error) = edit::set(document, &at, property, value.clone()) {
            self.status = format!("could not edit: {error}");
            return;
        }
        let text = document.text();
        match self.reload_text(&text) {
            Ok(()) => {
                if let Some((_, fields)) = &mut self.typed {
                    fields.retain(|(p, _)| *p != property);
                }
                self.status = format!("{} = {value}", tree::property_name(property));
            }
            Err(error) => {
                if let Some(document) = &mut self.document {
                    document.undo();
                }
                self.status = error;
            }
        }
    }

    /// Take the last edit back, or put it back again.
    pub(super) fn undo(&mut self, redo: bool) -> Task<Message> {
        let Some(document) = &mut self.document else {
            return Task::none();
        };
        let done = if redo {
            document.redo()
        } else {
            document.undo()
        };
        if !done {
            self.status = format!("nothing to {}", if redo { "redo" } else { "undo" });
            return Task::none();
        }
        let text = document.text();
        self.typed = None;
        self.status = match self.reload_text(&text) {
            Ok(()) => (if redo { "redone" } else { "undone" }).to_owned(),
            Err(error) => error,
        };
        Task::none()
    }

    /// The document's pointer to the layer at `path`, if the document
    /// has that layer where the show does.
    fn layer_pointer(&self, path: &LayerPath) -> Option<Pointer> {
        let session = self.session.as_ref()?;
        let engine = lock(&session.engine);
        let show = engine.show()?;
        let layer = tree::layer(show, path)?;
        let pointer = Pointer::parse(&tree::pointer(show, path)?).ok()?;
        let node = self.document.as_ref()?.get(&pointer)?.value();
        let named = node.get("name").or_else(|| node.get("id"))?;
        (named.as_str() == Some(layer.name.as_str())).then_some(pointer)
    }
}
