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
    Column, button, column, container, mouse_area, pick_list, rich_text, row, slider, space, span,
    text, text_input, toggler,
};
use iced::{Element, Fill, Size, Theme};

use super::{App, Message, Tab, theme};
use cuelight_editor_core::lists::List;
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
            return self.show_panel();
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
            if !tree::applies(property, &layer.kind) {
                continue;
            }
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
            let input = edit::input(property);
            let written = self.is_written(property);
            let error = self.typing_error(property);
            let (field, note): (Element<'a, Message>, String) = match (input, base) {
                (Some(input), Some(base)) => {
                    // A value the layer writes shows as itself; a default
                    // shows greyed in an empty field, and the note says so.
                    let shown = self.field(path, property);
                    let fallback = self.placeholder(property).unwrap_or_default();
                    let note = match (base == value, written) {
                        (true, true) => String::new(),
                        (true, false) => "default".to_owned(),
                        (false, _) => format!("{badge}: {value}"),
                    };
                    let marked = field_style(self.is_pending(path, property), error.is_some());
                    let typed = move || {
                        text_input(fallback, shown.unwrap_or_default())
                            .on_input(move |typed| Message::Type(property, typed))
                            .on_submit(Message::Apply(property))
                            .size(13)
                            .padding([1, 4])
                            .width(Fill)
                            .style(marked)
                    };
                    let field = match input {
                        edit::Input::Toggle => toggler(shown.unwrap_or(fallback) == "true")
                            .on_toggle(move |on| Message::Put(property, on.to_string()))
                            .size(14)
                            .boxed(),
                        edit::Input::Choice => match self.choices(show, path, property) {
                            Some(options) => {
                                pick_list(shown.map(str::to_owned), options, String::clone)
                                    .on_select(move |name| Message::Put(property, name))
                                    .placeholder(if fallback.is_empty() {
                                        "default".to_owned()
                                    } else {
                                        format!("default ({fallback})")
                                    })
                                    .text_size(13)
                                    .padding([1, 4])
                                    .width(Fill)
                                    .boxed()
                            }
                            // A layer picking from several is edited in
                            // its JSON for now.
                            None => text(value.clone()).size(13).width(Fill).boxed(),
                        },
                        edit::Input::Colour => row![
                            swatch(
                                shown.unwrap_or(fallback),
                                Message::Expand((!unfolded).then_some(property))
                            ),
                            typed()
                        ]
                        .spacing(4)
                        .align_y(iced::Center)
                        .boxed(),
                        edit::Input::Number | edit::Input::Text => typed().boxed(),
                    };
                    (field, note)
                }
                _ => (text(value).size(13).width(Fill).boxed(), badge),
            };
            panel = panel.push(
                row![
                    name,
                    field,
                    reset(written.then_some(Message::Reset(property))),
                    text(note).size(12)
                ]
                .spacing(6)
                .align_y(iced::Center)
                .boxed(),
            );
            if let Some(error) = error {
                panel = panel.push(problem(error));
            }
            if unfolded
                && input == Some(edit::Input::Colour)
                && let Some(shown) = self
                    .field(path, property)
                    .or_else(|| self.placeholder(property))
            {
                panel = panel.push(channels(
                    shown,
                    true,
                    move |typed| Message::Type(property, typed),
                    Message::Apply(property),
                ));
            }
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

        if !self.layer_fields.is_empty() {
            panel = panel.push(container(text("LAYER").size(12)).padding([6, 0]).boxed());
            for field in &self.layer_fields {
                panel = panel.push(self.field_row(field));
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

    /// One of the layer's other fields: its name, an editor fitting it,
    /// and whether the layer writes it or has the engine's default.
    fn field_row<'a>(&'a self, field: &'a super::editing::LayerField) -> Element<'a, Message> {
        use cuelight_editor_core::assets::Kind;
        use cuelight_editor_core::fields::Input;
        let label = field.field.label;
        let shown = self.field_text(label);
        let fallback = field.default.as_str();
        // A colour's label, like its swatch, unfolds its channels.
        let alpha = field.field.input == Input::Colour;
        let colour = field.editable && (alpha || field.field.input == Input::Opaque);
        let unfolded = colour && self.unfolded_field == Some(label);
        let unfold = Message::UnfoldField((!unfolded).then_some(label));
        let name: Element<'a, Message> = if colour {
            button(text(label).size(13))
                .on_press(unfold.clone())
                .width(86)
                .padding([2, 6])
                .style(if unfolded {
                    button::secondary
                } else {
                    button::text
                })
                .boxed()
        } else {
            container(text(label).size(13))
                .width(86)
                .padding([2, 6])
                .boxed()
        };
        let error = self.field_typing_error(label);
        let marked = field_style(self.is_field_pending(label), error.is_some());
        let words = |words: Vec<String>| {
            pick_list(shown.map(str::to_owned), words, String::clone)
                .on_select(move |word| Message::PutField(label, word))
                .placeholder(if fallback.is_empty() {
                    "default".to_owned()
                } else {
                    format!("default ({fallback})")
                })
                .text_size(13)
                .padding([1, 4])
                .width(Fill)
                .boxed()
        };
        let typed = || {
            text_input(fallback, shown.unwrap_or_default())
                .on_input(move |typed| Message::TypeField(label, typed))
                .on_submit(Message::ApplyField(label))
                .size(13)
                .padding([1, 4])
                .width(Fill)
                .style(marked)
        };
        let editor = if !field.editable {
            // A gradient, a list: shown as written, edited in the JSON.
            text(field.raw.clone()).size(12).width(Fill).boxed()
        } else {
            match field.field.input {
                Input::Choice(choices) => words(choices.iter().map(|w| (*w).to_owned()).collect()),
                Input::Artwork => words(
                    self.library
                        .iter()
                        .filter(|a| matches!(a.kind, Kind::Image | Kind::Vector))
                        .map(|a| a.name.clone())
                        .collect(),
                ),
                Input::Toggle => toggler(shown.unwrap_or(fallback) == "true")
                    .on_toggle(move |on| Message::PutField(label, on.to_string()))
                    .size(14)
                    .boxed(),
                Input::Colour | Input::Opaque => {
                    row![swatch(shown.unwrap_or(fallback), unfold), typed()]
                        .spacing(4)
                        .align_y(iced::Center)
                        .boxed()
                }
                // A share slides from 0 to 1, the number beside it.
                Input::Share => {
                    let now = shown
                        .unwrap_or(fallback)
                        .parse::<f32>()
                        .unwrap_or(0.0)
                        .clamp(0.0, 1.0);
                    row![
                        slider(0.0..=1.0, now, move |v: f32| {
                            Message::TypeField(label, format!("{:.2}", v))
                        })
                        .step(0.01)
                        .on_release(Message::ApplyField(label))
                        .width(Fill),
                        typed().width(52),
                    ]
                    .spacing(6)
                    .align_y(iced::Center)
                    .boxed()
                }
                Input::Number | Input::Count | Input::Pair | Input::Text => typed().boxed(),
            }
        };
        let note = if field.written { "" } else { "default" };
        let line = row![
            name,
            editor,
            reset(field.written.then_some(Message::ResetField(label))),
            text(note).size(12)
        ]
        .spacing(6)
        .align_y(iced::Center);
        let mut rows = column![line];
        if let Some(error) = error {
            rows = rows.push(problem(error));
        }
        if unfolded {
            rows = rows.push(channels(
                shown.unwrap_or(fallback),
                alpha,
                move |typed| Message::TypeField(label, typed),
                Message::ApplyField(label),
            ));
        }
        rows.boxed()
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

/// A field's border: amber while what is typed waits to be applied (Enter,
/// or moving on to another field), red while it does not read.
pub(super) fn field_style(
    pending: bool,
    invalid: bool,
) -> impl Fn(&Theme, text_input::Status) -> text_input::Style + Copy {
    move |theme, status| {
        let mut style = text_input::default(theme, status);
        let palette = theme.palette();
        let mark = if invalid {
            Some(palette.danger.base.color)
        } else if pending {
            Some(palette.warning.base.color)
        } else {
            None
        };
        if let Some(color) = mark {
            style.border.color = color;
            style.border.width = 1.5;
        }
        style
    }
}

/// Why what is typed does not read, under its row.
fn problem<'a>(error: String) -> Element<'a, Message> {
    container(text(error).size(11).style(text::danger))
        .padding([0, 92])
        .boxed()
}

/// The button that takes a written value back to its default, or the
/// room it takes, so the rows line up.
pub(super) fn reset<'a>(on_press: Option<Message>) -> Element<'a, Message> {
    match on_press {
        Some(message) => button(text("×").size(12))
            .on_press(message)
            .padding([0, 4])
            .style(button::text)
            .boxed(),
        None => space::horizontal().width(18).boxed(),
    }
}

/// A colour's four channels, `#RRGGBB` or `#RRGGBBAA`; none is white,
/// which is what an image without a tint looks like.
fn rgba(hex: &str) -> [u8; 4] {
    let digits = hex.trim_start_matches('#');
    let channel = |i: usize| {
        digits
            .get(i * 2..i * 2 + 2)
            .and_then(|pair| u8::from_str_radix(pair, 16).ok())
    };
    match digits.len() {
        6 | 8 => [
            channel(0).unwrap_or(255),
            channel(1).unwrap_or(255),
            channel(2).unwrap_or(255),
            channel(3).unwrap_or(255),
        ],
        _ => [255; 4],
    }
}

/// Four channels as the format writes a colour: no alpha when opaque.
fn hex([r, g, b, a]: [u8; 4]) -> String {
    if a == 255 {
        format!("#{r:02X}{g:02X}{b:02X}")
    } else {
        format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
    }
}

/// A square of the colour, on the pane's own background so its alpha
/// shows; an empty one for none. A click on it is `on_press`: its
/// channels unfolded or folded.
fn swatch<'a>(colour: &str, on_press: Message) -> Element<'a, Message> {
    let fill = (!colour.is_empty()).then(|| {
        let [r, g, b, a] = rgba(colour);
        iced::Color::from_rgba8(r, g, b, f32::from(a) / 255.0)
    });
    let square = container(space::horizontal().width(14))
        .width(18)
        .height(18)
        .style(move |theme: &Theme| container::Style {
            background: fill.map(iced::Background::Color),
            border: iced::Border {
                color: theme.palette().background.strong.color,
                width: 1.0,
                radius: 3.0.into(),
            },
            ..container::Style::default()
        });
    mouse_area(square)
        .on_press(on_press)
        .interaction(iced::mouse::Interaction::Pointer)
        .boxed()
}

/// Sliders for a colour's channels: the field and swatch follow while
/// dragging, as `typed` says, and letting go writes the colour
/// (`apply`).
fn channels<'a>(
    colour: &str,
    alpha: bool,
    typed: impl Fn(String) -> Message + Clone + 'a,
    apply: Message,
) -> Element<'a, Message> {
    let now = rgba(colour);
    let mut sliders = Column::<Element<'a, Message>>::new()
        .spacing(2)
        .padding([2, 18]);
    let names: &[&str] = if alpha {
        &["R", "G", "B", "A"]
    } else {
        &["R", "G", "B"]
    };
    for (i, name) in names.iter().copied().enumerate() {
        let value = now.get(i).copied().unwrap_or(255);
        sliders = sliders.push(
            row![
                text(name).size(12).width(14),
                slider(0.0..=255.0, f32::from(value), {
                    let typed = typed.clone();
                    move |v: f32| {
                        let mut next = now;
                        if !alpha {
                            next[3] = 255;
                        }
                        if let Some(channel) = next.get_mut(i) {
                            *channel = v.round().clamp(0.0, 255.0) as u8;
                        }
                        typed(hex(next))
                    }
                })
                .on_release(apply.clone())
                .width(Fill),
                text(value.to_string()).size(12).width(28),
            ]
            .spacing(6)
            .align_y(iced::Center)
            .boxed(),
        );
    }
    sliders.boxed()
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

impl App {
    /// The show itself, while nothing is picked: its settings and how it
    /// is output, edited like a layer's fields, then what it holds.
    fn show_panel(&self) -> Column<Element<'_, Message>> {
        if self.layer_fields.is_empty() {
            return summary(&self.summary);
        }
        let mut panel = Column::new().spacing(4).padding(12);
        let name = self.field_text("name").unwrap_or(&self.summary.name);
        panel = panel.push(text(name.to_owned()).size(16).boxed());
        panel = panel.push(
            text(format!("show, format {}", self.summary.format))
                .size(12)
                .boxed(),
        );
        // The settings by where they sit: the show's own, its output, the
        // dot matrix pass over it, and what it takes as input.
        let section = |path: &[&str]| match path {
            ["output", "passes", ..] => "DOTS",
            ["output", ..] => "OUTPUT",
            ["input", ..] => "INPUT",
            _ => "SHOW",
        };
        for heading in ["SHOW", "OUTPUT", "DOTS", "INPUT"] {
            panel = panel.push(container(text(heading).size(12)).padding([6, 0]).boxed());
            if heading == "DOTS" {
                panel = panel.push(
                    text("Each canvas pixel as a dot, as on a dot matrix panel: set any to turn it on, reset them all for none. The stage shows dots from 3 screen pixels per canvas pixel; zoom in on a large show.")
                        .size(12)
                        .boxed(),
                );
            }
            for field in &self.layer_fields {
                if section(field.field.path) == heading {
                    panel = panel.push(self.field_row(field));
                }
            }
        }
        panel = panel.push(self.list_panel(List::Keys));
        panel = panel.push(self.list_panel(List::Variables));
        panel = panel.push(container(text("CONTENTS").size(12)).padding([6, 0]).boxed());
        panel = panel.push(facts(&self.summary, &["show", "canvas"]).padding(0).boxed());
        // The show's own keys as written; its layers are each in their
        // own inspector.
        if let Some(document) = &self.document {
            panel = panel.push(container(text("JSON").size(12)).padding([6, 0]).boxed());
            panel = panel.push(
                text("layers and scenes counted, each layer's own in its inspector")
                    .size(12)
                    .boxed(),
            );
            let json = document.text_shortened(&["layers", "scenes"]);
            panel = panel.push(json_text(&json, &theme(self)));
        }
        panel
    }
}

fn summary(summary: &Summary) -> Column<Element<'_, Message>> {
    facts(summary, &[])
}

/// The show's facts as the open found them, but the lines in `leave`.
fn facts<'a>(summary: &'a Summary, leave: &[&str]) -> Column<Element<'a, Message>> {
    let mut rows = Column::new().spacing(6).padding(16);
    for (label, value) in opened::lines(summary) {
        if leave.contains(&label.as_str()) {
            continue;
        }
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
