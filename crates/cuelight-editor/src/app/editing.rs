//! Editing the picked layer from the inspector: what is typed into a
//! property's field goes into the document, the show reloads at the
//! playhead, and Ctrl+Z takes it back.

use super::*;
use cuelight_core::Influence;
use cuelight_editor_core::document::{Part, Pointer};
use cuelight_editor_core::edit;
use cuelight_editor_core::fields;
use cuelight_editor_core::scenes;

/// A field typed into: one of the picked layer's properties, one of
/// its other fields by label, or a row of one of the show's lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Typed {
    Property(Property),
    Field(&'static str),
    Row(cuelight_editor_core::lists::List, Option<String>),
    /// A trigger row of the picked scene.
    Trigger(Option<usize>),
}

/// A field of the picked layer as its row shows it.
#[derive(Debug, Clone)]
pub(super) struct LayerField {
    pub field: &'static fields::Field,
    /// Its value as the row edits it; `None` when the row cannot (a
    /// gradient, a list), and shows `raw` instead.
    pub shown: Option<String>,
    /// What the engine has when the layer writes nothing.
    pub default: String,
    pub raw: String,
    /// The row's editor can show the value (not a gradient or a list).
    pub editable: bool,
    /// The layer writes it; otherwise it is the engine's default.
    pub written: bool,
}

/// An edit held back because something other than the base owns the
/// property at the playhead: a new base would not show until it lets go.
#[derive(Debug, Clone)]
pub(super) struct Owned {
    /// The layer the inspector asks on.
    pub path: LayerPath,
    /// Each base value it would set, by layer and property: one typed
    /// value, or what a drag on the stage left.
    pub edits: Vec<(LayerPath, Property, serde_json::Value)>,
    /// What owns it, in words: `timeline enter`, `binding on speed`.
    pub owner: String,
}

impl Owned {
    /// The properties it would set, in words: `x and y`.
    pub fn names(&self) -> String {
        let mut names: Vec<String> = Vec::new();
        for (_, property, _) in &self.edits {
            let name = tree::property_name(*property);
            if !names.contains(&name) {
                names.push(name);
            }
        }
        names.join(" and ")
    }
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
            self.is_written(property)
                .then(|| self.placeholder(property))
                .flatten()
        })
    }

    /// A property's base value: what the layer writes, or its default,
    /// which an empty field shows greyed.
    pub(super) fn placeholder(&self, property: Property) -> Option<&str> {
        self.bases
            .iter()
            .find(|(p, _)| *p == property)
            .map(|(_, base)| base.as_str())
    }

    /// Whether the picked layer writes `property`, rather than leaving it
    /// to its default.
    pub(super) fn is_written(&self, property: Property) -> bool {
        self.written.contains(&property)
    }

    /// Take a property out of the picked layer, back to its default.
    pub(super) fn reset(&mut self, property: Property) -> Task<Message> {
        let Some(path) = self.selection.last().cloned() else {
            return Task::none();
        };
        let Some(at) = self.layer_pointer(&path) else {
            return Task::none();
        };
        let Some(document) = &mut self.document else {
            return Task::none();
        };
        if let Err(error) = edit::unset(document, &at, property) {
            self.status = format!("could not reset: {error}");
            return Task::none();
        }
        let text = document.text();
        self.finish_reset(&text, tree::property_name(property));
        if let Some((_, typed)) = &mut self.typed {
            typed.retain(|(p, _)| *p != property);
        }
        Task::none()
    }

    /// Take a field out of the picked layer (or the show), back to its
    /// default.
    pub(super) fn reset_field(&mut self, label: &'static str) -> Task<Message> {
        let Some(field) = self.field_named(label) else {
            return Task::none();
        };
        let Some(at) = self.fields_owner() else {
            return Task::none();
        };
        let Some(document) = &mut self.document else {
            return Task::none();
        };
        if let Err(error) = fields::unset(document, &at, field) {
            self.status = format!("could not reset: {error}");
            return Task::none();
        }
        let text = document.text();
        self.finish_reset(&text, label.to_owned());
        if let Some((_, typed)) = &mut self.field_typed {
            typed.retain(|(l, _)| *l != label);
        }
        Task::none()
    }

    /// Reload after a reset; a show that no longer loads takes it back.
    fn finish_reset(&mut self, text: &str, name: String) {
        match self.reload_text(text, true) {
            Ok(()) => self.status = format!("{name} back to its default"),
            Err(error) => {
                if let Some(document) = &mut self.document {
                    document.undo();
                }
                self.status = error;
            }
        }
    }

    /// The base values of the picked layer's editable properties, after
    /// whatever just changed.
    pub(super) fn refresh_bases(&mut self) {
        self.bases.clear();
        self.written.clear();
        let Some(path) = self.selection.last().cloned() else {
            return;
        };
        // Before the engine is held: finding the layer holds it too.
        if let (Some(at), Some(document)) = (self.layer_pointer(&path), &self.document) {
            self.written = tree::PROPERTIES
                .into_iter()
                .filter(|p| edit::written(document, &at, *p))
                .collect();
        }
        let Some(session) = &self.session else {
            return;
        };
        let path = &path;
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

    /// Apply what was typed and is still waiting, but in `keep`: moving
    /// on to another field, another layer or a save is what leaving a
    /// field is, since a field says nothing when it loses the focus. What
    /// does not read stays typed, marked, and is not applied.
    pub(super) fn commit_typed(&mut self, keep: Option<Typed>) {
        let properties: Vec<Property> = self
            .typed
            .as_ref()
            .filter(|(path, _)| self.selection.last() == Some(path))
            .map(|(_, typed)| typed.iter().map(|(p, _)| *p).collect())
            .unwrap_or_default();
        for property in properties {
            if keep == Some(Typed::Property(property)) || self.typing_error(property).is_some() {
                continue;
            }
            let _ = self.apply(property);
        }
        let labels: Vec<&'static str> = self
            .field_typed
            .as_ref()
            .filter(|(of, _)| *of == self.fields_of())
            .map(|(_, typed)| typed.iter().map(|(l, _)| *l).collect())
            .unwrap_or_default();
        for label in labels {
            if keep == Some(Typed::Field(label)) || self.field_typing_error(label).is_some() {
                continue;
            }
            let _ = self.apply_field(label);
        }
        self.commit_lists(keep.as_ref());
        let _ = self.apply_triggers(keep.as_ref());
        if self.style_name_typed.is_some() && keep.is_none() {
            let _ = self.rename_style();
        }
    }

    /// Whether anything typed is waiting to be applied, for the picked
    /// layer.
    pub(super) fn has_typed(&self) -> bool {
        let here = |path: &LayerPath| self.selection.last() == Some(path);
        self.typed
            .as_ref()
            .is_some_and(|(path, typed)| here(path) && !typed.is_empty())
            || self
                .field_typed
                .as_ref()
                .is_some_and(|(of, typed)| *of == self.fields_of() && !typed.is_empty())
            || (self.selection.is_empty() && !self.list_typed.is_empty())
            || !self.triggers_typed.is_empty()
            || self.style_name_typed.is_some()
    }

    /// Why what is typed into a property's field does not read, if it
    /// does not: what its row shows under it.
    pub(super) fn typing_error(&self, property: Property) -> Option<String> {
        let path = self.selection.last()?;
        let typed = self.typed(path, property)?;
        if typed.trim().is_empty() {
            return None;
        }
        edit::parse(property, typed).err()
    }

    /// The same for one of the layer's other fields.
    pub(super) fn field_typing_error(&self, label: &str) -> Option<String> {
        let (of, typed) = self.field_typed.as_ref()?;
        if *of != self.fields_of() {
            return None;
        }
        let (_, text) = typed.iter().find(|(l, _)| *l == label)?;
        if text.trim().is_empty() {
            return None;
        }
        let field = self.field_named(label)?;
        fields::parse(field, text).err()
    }

    /// Whether a property's field holds typing not applied yet.
    pub(super) fn is_pending(&self, path: &LayerPath, property: Property) -> bool {
        self.typed(path, property).is_some()
    }

    /// The same for one of the layer's other fields.
    pub(super) fn is_field_pending(&self, label: &str) -> bool {
        self.field_typed.as_ref().is_some_and(|(of, typed)| {
            *of == self.fields_of() && typed.iter().any(|(l, _)| *l == label)
        })
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
        // An emptied field takes the property back to its default.
        if typed.trim().is_empty() && edit::input(property) != Some(edit::Input::Colour) {
            return if self.is_written(property) {
                self.reset(property)
            } else {
                Task::none()
            };
        }
        let value = match edit::parse(property, &typed) {
            Ok(value) => value,
            Err(error) => {
                self.status = error;
                return Task::none();
            }
        };
        if let Some(owner) = self.owner(&path, property) {
            self.owned = Some(Owned {
                edits: vec![(path.clone(), property, value)],
                path,
                owner,
            });
            return Task::none();
        }
        self.edit_base(&path, property, value);
        Task::none()
    }

    /// What owns `property` of the layer at `path` at the playhead, in
    /// words, when it is not the base.
    pub(super) fn owner(&self, path: &LayerPath, property: Property) -> Option<String> {
        let session = self.session.as_ref()?;
        let engine = lock(&session.engine);
        match engine.explain(path, property).first() {
            Some(Influence::Timeline { timeline, .. }) => {
                Some(format!("timeline {}", timeline.name))
            }
            Some(Influence::Binding { variable, .. }) => Some(format!("a binding on {variable}")),
            _ => None,
        }
    }

    /// Set the base value anyway, after the question.
    pub(super) fn edit_owned(&mut self) -> Task<Message> {
        let Some(owned) = self.owned.take() else {
            return Task::none();
        };
        // What a drag left is one step, like the drag.
        if let Some(document) = &mut self.document {
            document.begin_step();
        }
        for (path, property, value) in owned.edits {
            self.edit_base(&path, property, value);
        }
        if let Some(document) = &mut self.document {
            document.end_step();
        }
        Task::none()
    }

    /// The picked layer's fields as their rows show them, after whatever
    /// just changed: what the layer writes, or the engine's default. With
    /// nothing picked, the show's settings.
    pub(super) fn refresh_fields_of_layer(&mut self) {
        self.layer_fields.clear();
        let Some(session) = &self.session else {
            return;
        };
        match self.fields_of() {
            FieldsOf::Style(name) => return self.refresh_style_fields(&name),
            FieldsOf::Scene(index) => return self.refresh_scene_fields(index),
            _ => {}
        }
        let Some(path) = self.selection.last() else {
            self.refresh_show_settings();
            return;
        };
        let Some(at) = self.layer_pointer(path) else {
            return;
        };
        let Some(written) = self
            .document
            .as_ref()
            .and_then(|d| d.get(&at))
            .map(|n| n.value())
        else {
            return;
        };
        let engine = lock(&session.engine);
        let read = engine
            .show()
            .and_then(|show| tree::layer(show, path))
            .and_then(|layer| serde_json::to_value(layer).ok())
            .unwrap_or_default();
        drop(engine);
        let Some(kind) = fields::kind(&written).map(str::to_owned) else {
            return;
        };
        for field in fields::of(&kind) {
            let own = fields::read(&written, field);
            let value = own.or_else(|| fields::read(&read, field));
            self.layer_fields.push(LayerField {
                field,
                shown: own.and_then(|v| fields::show(field, v)),
                default: fields::read(&read, field)
                    .and_then(|v| fields::show(field, v))
                    .unwrap_or_default(),
                raw: value.map(ToString::to_string).unwrap_or_default(),
                editable: value.is_none_or(|v| fields::show(field, v).is_some()),
                written: own.is_some(),
            });
        }
    }

    /// The show's settings as their rows show them: what the document
    /// writes, read key by key, or what the show has without it.
    fn refresh_show_settings(&mut self) {
        let Some(document) = &self.document else {
            return;
        };
        let root = Pointer::default();
        for field in fields::SHOW_FIELDS {
            let own = fields::written_value(document, &root, field);
            let default = fields::show_default(field);
            let value = own.as_ref().or(default.as_ref());
            self.layer_fields.push(LayerField {
                field,
                shown: own.as_ref().and_then(|v| fields::show(field, v)),
                default: default
                    .as_ref()
                    .and_then(|v| fields::show(field, v))
                    .unwrap_or_default(),
                raw: value.map(ToString::to_string).unwrap_or_default(),
                editable: value.is_none_or(|v| fields::show(field, v).is_some()),
                written: own.is_some(),
            });
        }
    }

    /// A scene's fields as their rows show them: what it writes, or what
    /// the show has, which is what it leaves out.
    fn refresh_scene_fields(&mut self, index: usize) {
        let Some(document) = &self.document else {
            return;
        };
        let at = scenes::pointer(index);
        if document.get(&at).is_none() {
            return;
        }
        for field in fields::SCENE_FIELDS {
            let own = fields::written_value(document, &at, field);
            let show = fields::SHOW_FIELDS
                .iter()
                .find(|f| f.path == field.path && field.path != ["name"]);
            let default = show.and_then(|show| {
                fields::written_value(document, &Pointer::default(), show)
                    .or_else(|| fields::show_default(show))
            });
            let value = own.as_ref().or(default.as_ref());
            self.layer_fields.push(LayerField {
                field,
                shown: own.as_ref().and_then(|v| fields::show(field, v)),
                default: default
                    .as_ref()
                    .and_then(|v| fields::show(field, v))
                    .unwrap_or_default(),
                raw: value.map(ToString::to_string).unwrap_or_default(),
                editable: value.is_none_or(|v| fields::show(field, v).is_some()),
                written: own.is_some(),
            });
        }
    }

    /// A font style's fields as their rows show them: what it writes, or
    /// what the engine reads without it.
    fn refresh_style_fields(&mut self, name: &str) {
        let Some(style) = self
            .document
            .as_ref()
            .and_then(|d| d.get(&style_pointer(name)))
            .map(|n| n.value())
        else {
            return;
        };
        for field in fields::STYLE_FIELDS {
            let own = fields::read(&style, field).cloned();
            let default = fields::style_default(&style, field);
            let value = own.as_ref().or(default.as_ref());
            self.layer_fields.push(LayerField {
                field,
                shown: own.as_ref().and_then(|v| fields::show(field, v)),
                default: default
                    .as_ref()
                    .and_then(|v| fields::show(field, v))
                    .unwrap_or_default(),
                raw: value.map(ToString::to_string).unwrap_or_default(),
                editable: value.is_none_or(|v| fields::show(field, v).is_some()),
                written: own.is_some(),
            });
        }
    }

    /// One of the rows' fields by its label, from the rows refreshed
    /// last: the engine's lock, which the inspector holds while it lays
    /// the rows out, is not taken.
    fn field_named(&self, label: &str) -> Option<&'static fields::Field> {
        self.layer_fields
            .iter()
            .find(|f| f.field.label == label)
            .map(|f| f.field)
    }

    /// What the inspector's fields are of now: the font style picked in
    /// the assets, the picked layer, or the show while nothing is.
    pub(super) fn fields_of(&self) -> FieldsOf {
        if self.tab == Tab::Assets
            && let Some(style) = &self.style
        {
            return FieldsOf::Style(style.clone());
        }
        match (self.selection.last(), self.scene) {
            (Some(path), _) => FieldsOf::Layer(path.clone()),
            (None, Some(index)) => FieldsOf::Scene(index),
            (None, None) => FieldsOf::Show,
        }
    }

    /// Where the fields being edited sit in the document.
    fn fields_owner(&self) -> Option<Pointer> {
        match self.fields_of() {
            FieldsOf::Layer(path) => self.layer_pointer(&path),
            FieldsOf::Show => Some(Pointer::default()),
            FieldsOf::Scene(index) => Some(scenes::pointer(index)),
            FieldsOf::Style(name) => Some(style_pointer(&name)),
        }
    }

    /// What a field's row shows: what was typed into it, or its value.
    pub(super) fn field_text(&self, label: &str) -> Option<&str> {
        if let Some((of, typed)) = &self.field_typed
            && *of == self.fields_of()
            && let Some((_, text)) = typed.iter().find(|(l, _)| *l == label)
        {
            return Some(text);
        }
        self.layer_fields
            .iter()
            .find(|f| f.field.label == label)
            .and_then(|f| f.shown.as_deref())
    }

    pub(super) fn type_into_field(&mut self, label: &'static str, text: String) {
        let path = self.fields_of();
        match &mut self.field_typed {
            Some((typed_for, typed)) if *typed_for == path => {
                typed.retain(|(l, _)| *l != label);
                typed.push((label, text));
            }
            _ => self.field_typed = Some((path, vec![(label, text)])),
        }
    }

    /// Enter in a field's row, a toggle flipped or a word picked: write
    /// it into the layer (or the show) and reload at the playhead.
    pub(super) fn apply_field(&mut self, label: &'static str) -> Task<Message> {
        let Some(field) = self.field_named(label) else {
            return Task::none();
        };
        let Some(typed) = self.field_text(label).map(str::to_owned) else {
            return Task::none();
        };
        // An emptied field takes it back to its default.
        if typed.trim().is_empty() {
            let written = self
                .layer_fields
                .iter()
                .any(|f| f.field.label == label && f.written);
            return if written {
                self.reset_field(label)
            } else {
                Task::none()
            };
        }
        let value = match fields::parse(field, &typed) {
            Ok(value) => value,
            Err(error) => {
                self.status = error;
                return Task::none();
            }
        };
        let Some(at) = self.fields_owner() else {
            return Task::none();
        };
        // A scene's name is told apart from the other scenes'.
        if let FieldsOf::Scene(index) = self.fields_of()
            && field.path == ["name"]
        {
            return self.rename_scene(index, &typed);
        }
        let of_show = self.fields_of() == FieldsOf::Show;
        let Some(document) = &mut self.document else {
            return Task::none();
        };
        // The default is not written down: setting it takes the key out.
        // The show's defaults need nothing of what it writes.
        let owner = if of_show {
            Some(serde_json::Value::Null)
        } else {
            document.get(&at).map(|layer| layer.value())
        };
        if owner.is_some_and(|owner| fields::is_default(&owner, field, &value)) {
            let written = fields::written(document, &at, field);
            if let Some((_, typed)) = &mut self.field_typed {
                typed.retain(|(l, _)| *l != label);
            }
            if written {
                return self.reset_field(label);
            }
            self.status = format!("{label} is its default");
            return Task::none();
        }
        if let Err(error) = fields::set(document, &at, field, value.clone()) {
            self.status = format!("could not edit: {error}");
            return Task::none();
        }
        let text = document.text();
        match self.reload_text(&text, true) {
            Ok(()) => {
                if let Some((_, typed)) = &mut self.field_typed {
                    typed.retain(|(l, _)| *l != label);
                }
                self.status = format!("{label} = {value}");
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

    pub(super) fn put_field(&mut self, label: &'static str, value: String) -> Task<Message> {
        self.type_into_field(label, value);
        self.apply_field(label)
    }

    /// A toggle flipped or a name picked: set it as if typed.
    pub(super) fn put(&mut self, property: Property, value: String) -> Task<Message> {
        self.type_into(property, value);
        self.apply(property)
    }

    /// What a list offers for `property` of the layer at `path`: the
    /// show's font styles, or its sounds or clips. `None` while the layer
    /// picks from several, which the list cannot show.
    pub(super) fn choices(
        &self,
        show: &cuelight_core::Show,
        path: &LayerPath,
        property: Property,
    ) -> Option<Vec<String>> {
        use cuelight_editor_core::assets::Kind;
        let kind = match property {
            Property::Font => return Some(show.fonts.keys().cloned().collect()),
            Property::Sound => Kind::Sound,
            Property::Video => Kind::Video,
            _ => return None,
        };
        // The inspector calls this with the engine held: the show it
        // passes is all there is to go by.
        let at = edit::pointer(&self.pointer_in(show, path)?, property)?;
        if self
            .document
            .as_ref()
            .and_then(|d| d.get(&at))
            .is_some_and(|node| node.value().is_array())
        {
            return None;
        }
        Some(
            self.library
                .iter()
                .filter(|asset| asset.kind == kind)
                .map(|asset| asset.name.clone())
                .collect(),
        )
    }

    /// Write `value` as the base of `property` on the layer at `path`
    /// and reload at the playhead; a show that no longer loads takes the
    /// edit back.
    fn edit_base(&mut self, path: &LayerPath, property: Property, value: serde_json::Value) {
        self.write_base(path, property, value, true);
    }

    /// `edit_base`, with or without auditing the document after it.
    fn write_base(
        &mut self,
        path: &LayerPath,
        property: Property,
        value: serde_json::Value,
        audit: bool,
    ) {
        let Some(at) = self.layer_pointer(path) else {
            self.status = "the document does not have this layer where the show does".to_owned();
            return;
        };
        let Some(document) = &mut self.document else {
            return;
        };
        // The default is not written down: setting it takes the key out.
        // A drag (no audit) writes freely and settles this when it ends.
        let default = audit
            && document
                .get(&at)
                .is_some_and(|layer| edit::is_default(&layer.value(), property, &value));
        if default {
            let written = edit::written(document, &at, property);
            if let Some((_, fields)) = &mut self.typed {
                fields.retain(|(p, _)| *p != property);
            }
            if written {
                let _ = self.reset(property);
            } else {
                self.status = format!("{} is its default", tree::property_name(property));
            }
            return;
        }
        if let Err(error) = edit::set(document, &at, property, value.clone()) {
            self.status = format!("could not edit: {error}");
            return;
        }
        let text = document.text();
        match self.reload_text(&text, audit) {
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
        self.status = match self.reload_text(&text, true) {
            Ok(()) => (if redo { "redone" } else { "undone" }).to_owned(),
            Err(error) => error,
        };
        Task::none()
    }

    /// The document's pointer to the layer at `path`, if the document
    /// has that layer where the show does.
    pub(super) fn layer_pointer(&self, path: &LayerPath) -> Option<Pointer> {
        let session = self.session.as_ref()?;
        let engine = lock(&session.engine);
        self.pointer_in(engine.show()?, path)
    }

    /// `layer_pointer` for a caller that holds the engine already: the
    /// engine's lock is not taken twice.
    fn pointer_in(&self, show: &cuelight_core::Show, path: &LayerPath) -> Option<Pointer> {
        let layer = tree::layer(show, path)?;
        let pointer = Pointer::parse(&tree::pointer(show, path)?).ok()?;
        let node = self.document.as_ref()?.get(&pointer)?.value();
        let named = node.get("name").or_else(|| node.get("id"))?;
        (named.as_str() == Some(layer.name.as_str())).then_some(pointer)
    }
}

/// What the inspector's fields are of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum FieldsOf {
    Show,
    Layer(LayerPath),
    /// A scene, by its place in the show's scenes.
    Scene(usize),
    /// A font style, by its name in the show's `fonts`.
    Style(String),
}

/// The document's pointer to the font style `name`.
pub(super) fn style_pointer(name: &str) -> Pointer {
    Pointer::default()
        .then(Part::Key("fonts".to_owned()))
        .then(Part::Key(name.to_owned()))
}

/// A number being dragged by its label: where the drag started, and the
/// value it started from.
#[derive(Debug, Clone)]
pub(super) struct Scrub {
    pub path: LayerPath,
    pub property: Property,
    pub from: f64,
    /// The cursor's x when the drag began; the first move tells it.
    pub start: Option<f32>,
    /// The cursor has gone far enough for this to be a drag, not a click.
    pub dragging: bool,
    /// Where the cursor is now; applied once a frame, so moves that come
    /// faster than the show reloads fold into one.
    pub at: f32,
    /// The value the drag last wrote.
    pub applied: Option<serde_json::Value>,
}

/// How far a drag must go before it is one, in logical pixels.
const SLOP: f32 = 3.0;

/// How much one pixel of drag changes a property: a pixel for places,
/// degrees and frames, a hundredth for what runs from 0 to 1.
fn step(property: Property) -> f64 {
    match property {
        Property::Opacity
        | Property::Scale
        | Property::ScaleX
        | Property::ScaleY
        | Property::Reveal
        | Property::Gain
        | Property::Pan => 0.01,
        _ => 1.0,
    }
}

impl App {
    /// The mouse went down on a property's label.
    pub(super) fn scrub_start(&mut self, property: Property) -> Task<Message> {
        let Some(path) = self.selection.last().cloned() else {
            return Task::none();
        };
        let from = self
            .bases
            .iter()
            .find(|(p, _)| *p == property)
            .and_then(|(_, base)| base.parse::<f64>().ok());
        self.scrub = Some(Scrub {
            path,
            property,
            from: from.unwrap_or_default(),
            start: None,
            dragging: false,
            at: 0.0,
            applied: None,
        });
        Task::none()
    }

    /// The mouse moved while a label is held: note where it is, and once
    /// it has gone past the slop, start the drag's undo step. The value
    /// follows on the next frame.
    pub(super) fn scrub_move(&mut self, x: f32) -> Task<Message> {
        let Some(scrub) = &mut self.scrub else {
            return Task::none();
        };
        let start = *scrub.start.get_or_insert(x);
        scrub.at = x;
        if scrub.dragging || (x - start).abs() < SLOP {
            return Task::none();
        }
        let (path, property) = (scrub.path.clone(), scrub.property);
        if edit::input(property) != Some(edit::Input::Number) {
            self.scrub = None;
            return Task::none();
        }
        if let Some(owner) = self.owner(&path, property) {
            self.status = format!(
                "{} is set by {owner} at the playhead: type a value to change its base",
                tree::property_name(property)
            );
            self.scrub = None;
            return Task::none();
        }
        if let Some(scrub) = &mut self.scrub {
            scrub.dragging = true;
        }
        if let Some(document) = &mut self.document {
            document.begin_step();
        }
        Task::none()
    }

    /// A frame while dragging: write the value the cursor is at, if it
    /// is not the one written last. The audit waits for the drag to end.
    pub(super) fn scrub_apply(&mut self) -> Task<Message> {
        let Some(Scrub {
            path,
            property,
            from,
            start: Some(start),
            dragging: true,
            at,
            applied,
        }) = self.scrub.clone()
        else {
            return Task::none();
        };
        let step = step(property);
        let raw = from + f64::from(at - start) * step;
        // Whole steps, so a drag writes 0.43 and not 0.4300000000001.
        let value = (raw / step).round() * step;
        let value = if step < 1.0 {
            serde_json::Value::from((value * 100.0).round() / 100.0)
        } else {
            serde_json::Value::from(value as i64)
        };
        if applied.as_ref() == Some(&value) {
            return Task::none();
        }
        self.write_base(&path, property, value.clone(), false);
        if let Some(scrub) = &mut self.scrub {
            scrub.applied = Some(value);
        }
        Task::none()
    }

    /// The mouse came up: a drag ends its step, a click unfolds the row.
    pub(super) fn scrub_end(&mut self) -> Task<Message> {
        let Some(scrub) = self.scrub.take() else {
            return Task::none();
        };
        if scrub.dragging {
            // Where the mouse came up is where it ends, then the audit
            // the drag left out.
            let (path, property) = (scrub.path.clone(), scrub.property);
            self.scrub = Some(scrub);
            let _ = self.scrub_apply();
            self.scrub = None;
            // A drag that ends on the default leaves the key out, in the
            // same step.
            let at = self.layer_pointer(&path);
            let ends_default = at.as_ref().is_some_and(|at| {
                let Some(document) = &self.document else {
                    return false;
                };
                let here = edit::pointer(at, property).and_then(|here| document.get(&here));
                match (document.get(at), here) {
                    (Some(layer), Some(now)) => {
                        edit::is_default(&layer.value(), property, &now.value())
                    }
                    _ => false,
                }
            });
            if ends_default
                && let (Some(at), Some(document)) = (at, &mut self.document)
                && edit::unset(document, &at, property).is_ok()
            {
                let text = document.text();
                let _ = self.reload_text(&text, false);
            }
            if let Some(document) = &mut self.document {
                document.end_step();
            }
            if let (Some(session), Some(document)) = (&mut self.session, &self.document) {
                session.audit(&document.text());
            }
        } else {
            self.expanded = (self.expanded != Some(scrub.property)).then_some(scrub.property);
        }
        Task::none()
    }
}
