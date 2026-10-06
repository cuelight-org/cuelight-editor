//! The inspector: a picked layer's values, where they come from, and
//! its JSON; the show's own facts while nothing is picked.

use cuelight_core::{Influence, Layer, LayerKind, LayerPath, Property, TimelineOwner, Value};
use cuelight_editor_core::document::Pointer;
use cuelight_editor_core::edit;
use cuelight_editor_core::inputs;
use cuelight_editor_core::syntax::{self, Token};
use cuelight_editor_core::tree;
use iced::widget::Widget as _;
use iced::widget::text::Span;
use iced::widget::{
    Column, button, column, container, mouse_area, rich_text, row, span, text, text_input,
};
use iced::{Element, Fill, Size, Theme};

use super::{App, Message, Tab, theme};
use cuelight_editor_core::opened::{self, Summary};
use cuelight_editor_core::session::{Session, lock};

impl App {
    /// The inspector: the picked layer's properties with their live
    /// values and where each comes from, its bindings and timelines, and
    /// its JSON; the show's own facts while nothing is picked. With the
    /// assets showing, the picked asset.
    pub(super) fn inspector_panel<'a>(
        &'a self,
        session: &'a Session,
        size: Size,
    ) -> Column<Element<'a, Message>> {
        if self.tab == Tab::Assets
            && let Some(i) = self.selected
        {
            return self.asset_panel(session, i, size);
        }
        let Some(path) = self.selection.last() else {
            return summary(&self.summary);
        };
        let engine = lock(&session.engine);
        let Some(show) = engine.show() else {
            return summary(&self.summary);
        };
        let Some(layer) = tree::layer(show, path) else {
            return Column::new().push(text("the picked layer is gone").size(14).boxed());
        };
        let mut panel = Column::new().spacing(4).padding(12);
        panel = panel.push(text(layer.name.clone()).size(16).boxed());
        panel = panel.push(
            text(format!(
                "{}, {}",
                tree::kind_name(&layer.kind),
                tree::describe(show, path)
            ))
            .size(12)
            .boxed(),
        );
        if let LayerKind::Part { id, pivot } = &layer.kind {
            // An element of the artwork above it, moved in the artwork's
            // own coordinates around its pivot.
            let around = match pivot {
                Some([x, y]) => format!("pivot {x}, {y}"),
                None => "pivot at the centre of its bounds".to_owned(),
            };
            panel = panel.push(
                text(format!("element {id:?} of the artwork, {around}"))
                    .size(12)
                    .boxed(),
            );
        }
        if self.selection.len() > 1 {
            panel = panel.push(
                text(format!("{} picked", self.selection.len()))
                    .size(12)
                    .boxed(),
            );
        }
        if let Some(owned) = self.owned.as_ref().filter(|owned| owned.path == *path) {
            panel = panel.push(
                container(
                    column![
                        text(format!(
                            "{} is set by {} at the playhead: a new base value shows once it lets go.",
                            tree::property_name(owned.property),
                            owned.owner
                        ))
                        .size(12),
                        row![
                            button(text("Edit base").size(12)).on_press(Message::EditOwned),
                            // Keying waits for the timelines (spec item 34).
                            button(text("Key it").size(12)),
                            button(text("Cancel").size(12)).on_press(Message::KeepOwned),
                        ]
                        .spacing(6),
                    ]
                    .spacing(4),
                )
                .padding(6)
                .style(container::bordered_box).boxed(),
            );
        }

        // Every property the layer has, its value now, and its sources.
        let live: Vec<(Property, Value)> = engine
            .values()
            .unwrap_or_default()
            .into_iter()
            .filter(|v| v.layer == *path)
            .map(|v| (v.property, v.value))
            .collect();
        let mut owned = Vec::new();
        let mut heading = "";
        for property in tree::PROPERTIES {
            let sources = engine.explain(path, property);
            if sources.is_empty() {
                continue;
            }
            let group = match property {
                Property::X
                | Property::Y
                | Property::Rotation
                | Property::Scale
                | Property::ScaleX
                | Property::ScaleY => "PLACEMENT",
                _ => "APPEARANCE",
            };
            if group != heading {
                heading = group;
                panel = panel.push(container(text(group).size(12)).padding([6, 0]).boxed());
            }
            let value = live
                .iter()
                .find(|(p, _)| *p == property)
                .map(|(_, v)| now(v))
                .or_else(|| sources.iter().find_map(influence_value))
                .unwrap_or_default();
            let badge = sources.first().map(winner).unwrap_or_default();
            for source in &sources {
                if let Influence::Timeline {
                    timeline,
                    held,
                    local,
                    ..
                } = source
                    && let TimelineOwner::Layer(owner) = &timeline.owner
                    && owner == path
                {
                    owned.push((timeline.index, *held, *local));
                }
            }
            let unfolded = self.expanded == Some(property);
            // A number's label drags its value; a click on any label
            // unfolds where the value comes from.
            let name: Element<'a, Message> = if edit::input(property) == Some(edit::Input::Number) {
                mouse_area(
                    container(text(tree::property_name(property)).size(13))
                        .width(86)
                        .padding([2, 6])
                        .style(if unfolded {
                            container::rounded_box
                        } else {
                            container::transparent
                        }),
                )
                .on_press(Message::ScrubStart(property))
                .interaction(iced::mouse::Interaction::ResizingHorizontally)
                .boxed()
            } else {
                button(text(tree::property_name(property)).size(13))
                    .on_press(Message::Expand((!unfolded).then_some(property)))
                    .width(86)
                    .padding([2, 6])
                    .style(if unfolded {
                        button::secondary
                    } else {
                        button::text
                    })
                    .boxed()
            };
            // An editable property shows its base, which an edit changes;
            // what wins now, if something else does, goes by the badge.
            let base = sources.iter().find_map(|source| match source {
                Influence::Base { value } => Some(inputs::show_value(value)),
                _ => None,
            });
            let (field, note): (Element<'a, Message>, String) = match (edit::input(property), base)
            {
                (Some(_), Some(base)) => {
                    let shown = self.field(path, property).unwrap_or_default();
                    let note = if base == value {
                        badge
                    } else {
                        format!("{badge}: {value}")
                    };
                    (
                        text_input("", shown)
                            .on_input(move |typed| Message::Type(property, typed))
                            .on_submit(Message::Apply(property))
                            .size(13)
                            .padding([1, 4])
                            .width(Fill)
                            .boxed(),
                        note,
                    )
                }
                _ => (text(value).size(13).width(Fill).boxed(), badge),
            };
            panel = panel.push(
                row![name, field, text(note).size(12)]
                    .spacing(6)
                    .align_y(iced::Center)
                    .boxed(),
            );
            if unfolded {
                for (rank, source) in sources.iter().enumerate() {
                    panel = panel.push(
                        container(
                            text(format!("{}. {}", rank + 1, describe_source(source))).size(12),
                        )
                        .padding([0, 18])
                        .boxed(),
                    );
                }
            }
        }

        if !layer.bindings.is_empty() {
            panel = panel.push(container(text("BINDINGS").size(12)).padding([6, 0]).boxed());
            for binding in &layer.bindings {
                let mut line = format!(
                    "{} <- {}",
                    tree::property_name(binding.property),
                    binding.reading.variable
                );
                if binding.reading.map.is_some() {
                    line.push_str(", mapped");
                }
                if let Some(threshold) = binding.reading.threshold {
                    line.push_str(&format!(", from {threshold}"));
                }
                if let Some(debounce) = binding.reading.debounce {
                    line.push_str(&format!(", settled {debounce} s"));
                }
                if binding.scale != 1.0 {
                    line.push_str(&format!(", x {}", binding.scale));
                }
                if binding.offset != 0.0 {
                    line.push_str(&format!(", + {}", binding.offset));
                }
                if let Some(transition) = &binding.transition {
                    line.push_str(&format!(", over {} s", transition.duration));
                }
                panel = panel.push(text(line).size(13).boxed());
            }
        }

        if !layer.timelines.is_empty() {
            panel = panel.push(
                container(text("TIMELINES").size(12))
                    .padding([6, 0])
                    .boxed(),
            );
            for (index, timeline) in layer.timelines.iter().enumerate() {
                let mut starts: Vec<String> =
                    timeline.trigger.iter().map(|t| format!("on {t}")).collect();
                if let Some(when) = &timeline.when {
                    starts.push(format!("when {}", when.variable));
                }
                if let Some(whilst) = &timeline.whilst {
                    starts.push(format!("while {}", whilst.variable));
                }
                if timeline.autoplay {
                    starts.push("at load".to_owned());
                }
                let tracks: Vec<String> = timeline
                    .tracks
                    .iter()
                    .map(|t| tree::property_name(t.property))
                    .collect();
                let state = owned
                    .iter()
                    .find(|(i, _, _)| *i == index)
                    .map(|(_, held, local)| match (held, local) {
                        (true, _) => "held".to_owned(),
                        (false, Some(at)) => format!("running, {at:.2} s"),
                        (false, None) => "running".to_owned(),
                    })
                    .unwrap_or_default();
                panel = panel.push(
                    row![
                        text(timeline.name.clone()).size(13).width(Fill),
                        text(state).size(12)
                    ]
                    .spacing(8)
                    .boxed(),
                );
                panel = panel.push(
                    container(
                        text(format!(
                            "{}{}: {}",
                            starts.join(", "),
                            if timeline.looping { ", looping" } else { "" },
                            tracks.join(", ")
                        ))
                        .size(12),
                    )
                    .padding([0, 12])
                    .boxed(),
                );
            }
        }

        panel = panel.push(container(text("JSON").size(12)).padding([6, 0]).boxed());
        let json = match self.written(show, path, layer) {
            Some(written) => written,
            None => {
                // The document does not have the layer where the engine
                // does: what the engine read, defaults and all.
                panel = panel.push(text("as the engine read it").size(12).boxed());
                serde_json::to_string_pretty(layer).unwrap_or_default()
            }
        };
        panel = panel.push(json_text(&json, &theme(self)));
        panel
    }

    /// The layer's text as the document has it, if the document has that
    /// layer at the layer's place.
    pub(super) fn written(
        &self,
        show: &cuelight_core::Show,
        path: &LayerPath,
        layer: &Layer,
    ) -> Option<String> {
        let document = self.document.as_ref()?;
        let pointer = Pointer::parse(&tree::pointer(show, path)?).ok()?;
        let node = document.get(&pointer)?.value();
        // A part is named by its id.
        let named = node.get("name").or_else(|| node.get("id"))?;
        (named.as_str() == Some(layer.name.as_str())).then(|| document.text_at(&pointer))?
    }
}

/// JSON in the editor's mono font, coloured by token from the theme's
/// palette: keys, strings, numbers, literals and punctuation each their
/// own, whitespace and anything else in the text's colour.
fn json_text<'a>(json: &str, theme: &Theme) -> Element<'a, Message> {
    let palette = theme.palette();
    let text_l = palette.background.base.text.into_oklch().l;
    let back_l = palette.background.base.color.into_oklch().l;
    // A colour at a lightness this far from the text's towards the
    // background's, so it reads on the pane as text does, light on dark
    // or dark on light, whichever the theme is.
    let toward = |c: iced::Color, far: f32| {
        let mut oklch = c.into_oklch();
        oklch.l = text_l + (back_l - text_l) * far;
        Some(iced::Color::from_oklch(oklch))
    };
    // The palette's hues are picked to fill buttons; as text they are
    // lifted near the text's lightness. Dark text on light needs more
    // room for its hue to show.
    let near = if palette.is_dark { 0.25 } else { 0.4 };
    let colour = |token: Token| match token {
        Token::Key => toward(palette.primary.base.color, near),
        Token::String => toward(palette.success.base.color, near),
        Token::Number => toward(palette.warning.base.color, near),
        Token::Literal => toward(palette.danger.base.color, near),
        // The text's grey, halfway to the background: seen, not read.
        Token::Punctuation => toward(palette.background.base.text, 0.5),
        Token::Plain => None,
    };
    let spans: Vec<Span<'a, ()>> = syntax::tokens(json)
        .into_iter()
        .map(|(token, range)| span(json[range].to_owned()).color_maybe(colour(token)))
        .collect();
    rich_text(spans)
        .size(12)
        .font(iced::Font::new("DM Mono"))
        .boxed()
}

/// A value as it is now, numbers to two places: a timeline or a
/// transition moving it leaves long fractions no one reads.
fn now(value: &Value) -> String {
    match value {
        Value::Number(n) => {
            let rounded = format!("{n:.2}");
            let rounded = rounded.trim_end_matches('0').trim_end_matches('.');
            if rounded == "-0" {
                "0".to_owned()
            } else {
                rounded.to_owned()
            }
        }
        _ => inputs::show_value(value),
    }
}

/// What an influence hands the property, as text.
fn influence_value(influence: &Influence) -> Option<String> {
    match influence {
        Influence::Base { value } => Some(inputs::show_value(value)),
        Influence::Binding { value, .. } => value.as_ref().map(inputs::show_value),
        Influence::Timeline { value, .. } => value.map(|v| inputs::show_value(&Value::Number(v))),
        _ => None,
    }
}

/// The badge of the source that wins right now.
fn winner(influence: &Influence) -> String {
    match influence {
        Influence::Base { .. } => "base".to_owned(),
        Influence::Binding { variable, .. } => format!("bound to {variable}"),
        Influence::Timeline { timeline, held, .. } => {
            if *held {
                format!("held by {}", timeline.name)
            } else {
                format!("timeline {}", timeline.name)
            }
        }
        _ => "?".to_owned(),
    }
}

/// One source of a property's value, in the unfolded list.
fn describe_source(influence: &Influence) -> String {
    let value = influence_value(influence)
        .map(|v| format!(" = {v}"))
        .unwrap_or_default();
    match influence {
        Influence::Base { .. } => format!("base{value}"),
        Influence::Binding {
            index, variable, ..
        } => format!("binding {} on {variable}{value}", index + 1),
        Influence::Timeline {
            timeline,
            local,
            held,
            ..
        } => {
            let at = match (held, local) {
                (true, _) => ", held".to_owned(),
                (false, Some(t)) => format!(" at {t:.2} s"),
                (false, None) => String::new(),
            };
            format!("timeline {}{at}{value}", timeline.name)
        }
        _ => format!("another source{value}"),
    }
}

fn summary(summary: &Summary) -> Column<Element<'_, Message>> {
    let mut rows = Column::new().spacing(6).padding(16);
    for (label, value) in opened::lines(summary) {
        rows = rows.push(
            column![text(label).size(12), text(value).size(14)]
                .spacing(2)
                .boxed(),
        );
    }
    if !summary.problems.is_empty() {
        rows = rows.push(
            text(format!("{} problem(s)", summary.problems.len()))
                .size(14)
                .boxed(),
        );
        for problem in &summary.problems {
            rows = rows.push(text(problem).size(13).boxed());
        }
    }
    rows
}
