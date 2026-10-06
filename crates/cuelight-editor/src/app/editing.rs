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
        if let Some(owner) = self.owner(&path, property) {
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
        let at = edit::pointer(&self.layer_pointer(path)?, property)?;
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
        | Property::Gain => 0.01,
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
            self.scrub = Some(scrub);
            let _ = self.scrub_apply();
            self.scrub = None;
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
