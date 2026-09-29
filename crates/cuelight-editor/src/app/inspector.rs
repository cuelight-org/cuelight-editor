//! The inspector: a picked layer's values, where they come from, and
//! its JSON; the show's own facts while nothing is picked.

use cuelight_core::{Influence, Layer, LayerKind, LayerPath, Property, TimelineOwner, Value};
use cuelight_editor_core::document::Pointer;
use cuelight_editor_core::inputs;
use cuelight_editor_core::syntax::{self, Token};
use cuelight_editor_core::tree;
use iced::widget::text::Span;
use iced::widget::{Column, button, column, container, rich_text, row, span, text};
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
    ) -> Column<'a, Message> {
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
            return Column::new().push(text("the picked layer is gone").size(14));
        };
        let mut panel = Column::new().spacing(4).padding(12);
        panel = panel.push(text(layer.name.clone()).size(16));
        panel = panel.push(
            text(format!(
                "{}, {}",
                tree::kind_name(&layer.kind),
                tree::describe(show, path)
            ))
            .size(12),
        );
        if let LayerKind::Part { id, pivot } = &layer.kind {
            // An element of the artwork above it, moved in the artwork's
            // own coordinates around its pivot.
            let around = match pivot {
                Some([x, y]) => format!("pivot {x}, {y}"),
                None => "pivot at the centre of its bounds".to_owned(),
            };
            panel = panel.push(text(format!("element {id:?} of the artwork, {around}")).size(12));
        }
        if self.selection.len() > 1 {
            panel = panel.push(text(format!("{} picked", self.selection.len())).size(12));
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
                panel = panel.push(container(text(group).size(12)).padding([6, 0]));
            }
            let value = live
                .iter()
                .find(|(p, _)| *p == property)
                .map(|(_, v)| inputs::show_value(v))
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
            let line = row![
                text(tree::property_name(property)).size(13).width(80),
                text(value).size(13).width(Fill),
                text(badge).size(12),
            ]
            .spacing(8)
            .align_y(iced::Center);
            let mut b = button(line)
                .on_press(Message::Expand((!unfolded).then_some(property)))
                .width(Fill)
                .padding([2, 6])
                .style(button::text);
            if unfolded {
                b = b.style(button::secondary);
            }
            panel = panel.push(b);
            if unfolded {
                for (rank, source) in sources.iter().enumerate() {
                    panel = panel.push(
                        container(
                            text(format!("{}. {}", rank + 1, describe_source(source))).size(12),
                        )
                        .padding([0, 18]),
                    );
                }
            }
        }

        if !layer.bindings.is_empty() {
            panel = panel.push(container(text("BINDINGS").size(12)).padding([6, 0]));
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
                panel = panel.push(text(line).size(13));
            }
        }

        if !layer.timelines.is_empty() {
            panel = panel.push(container(text("TIMELINES").size(12)).padding([6, 0]));
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
                    .spacing(8),
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
                    .padding([0, 12]),
                );
            }
        }

        panel = panel.push(container(text("JSON").size(12)).padding([6, 0]));
        let json = match self.written(show, path, layer) {
            Some(written) => written,
            None => {
                // The document does not have the layer where the engine
                // does: what the engine read, defaults and all.
                panel = panel.push(text("as the engine read it").size(12));
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
        .into()
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

fn summary(summary: &Summary) -> Column<'_, Message> {
    let mut rows = Column::new().spacing(6).padding(16);
    for (label, value) in opened::lines(summary) {
        rows = rows.push(column![text(label).size(12), text(value).size(14)].spacing(2));
    }
    if !summary.problems.is_empty() {
        rows = rows.push(text(format!("{} problem(s)", summary.problems.len())).size(14));
        for problem in &summary.problems {
            rows = rows.push(text(problem).size(13));
        }
    }
    rows
}
