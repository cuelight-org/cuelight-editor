//! The assets: the list in the library, and a picked one's preview,
//! specimen and uses in the inspector.

use std::collections::BTreeMap;

use cuelight_editor_core::assets::{self, Asset, Kind};
use cuelight_editor_core::specimen::{self, Drawn, Sizing};
use iced::widget::Widget as _;
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::text::Wrapping;
use iced::widget::{Column, button, column, container, image, row, scrollable, space, svg, text};
use iced::{ContentFit, Element, Fill, Font, Size, Task};

use super::{App, Face, Faces, Message, Thumb};
use cuelight_editor_core::session::{Session, lock};

impl App {
    /// The assets by kind, each with its thumbnail, its format and how
    /// often the show uses it; what else there is to know of one is in
    /// the inspector once it is picked.
    pub(super) fn assets_panel<'a>(&'a self) -> Column<Element<'a, Message>> {
        const THUMB: f32 = 40.0;
        let mut panel = Column::new().spacing(4).padding(12);
        if self.library.is_empty() {
            return panel.push(text("This show ships no assets.").size(14).boxed());
        }
        let mut heading: Option<&str> = None;
        for (i, asset) in self.library.iter().enumerate() {
            if heading != Some(asset.kind.heading()) {
                heading = Some(asset.kind.heading());
                panel = panel.push(text(asset.kind.heading()).size(12).boxed());
            }
            let thumb: Element<'a, Message> = match self.thumbs.get(i).and_then(Option::as_ref) {
                Some(Thumb::Image(handle)) => image(handle.clone())
                    .width(THUMB)
                    .height(THUMB)
                    .content_fit(ContentFit::Contain)
                    .filter_method(image::FilterMethod::Nearest)
                    .boxed(),
                Some(Thumb::Svg(handle)) => svg(handle.clone())
                    .width(THUMB)
                    .height(THUMB)
                    .content_fit(ContentFit::Contain)
                    .boxed(),
                None => container(text(kind_mark(asset.kind)).size(16))
                    .width(THUMB)
                    .height(THUMB)
                    .center_x(THUMB)
                    .center_y(THUMB)
                    .boxed(),
            };
            let mut facts = asset.summary();
            let playing = self.preview.as_ref().is_some_and(|p| p.index == i);
            if playing {
                facts.push_str(", playing");
            }
            let mut about = column![text(&asset.name).size(14), text(facts).size(12)].spacing(2);
            if let Some(Some(faces)) = self.faces.get(i) {
                // The sample at the size the show uses the font at, as
                // tall as a row allows; what does not fit is cut off.
                about = about.push(
                    container(self.face(&faces.sample, specimen::SAMPLE, 1.0, Some(SAMPLE_HEIGHT)))
                        .width(Fill)
                        .clip(true)
                        .boxed(),
                );
            }
            let line = row![thumb, about].spacing(8).align_y(iced::Center);
            let mut b = button(line)
                .on_press(Message::Select(Some(i)))
                .width(Fill)
                .style(button::text);
            if self.selected == Some(i) {
                b = b.style(button::secondary);
            }
            if asset.kind == Kind::Sound {
                // Played once from here, outside the show's clock; the
                // same press stops it. Nothing to press when there is no
                // sound to be had.
                let mut play = button(text(if playing { "stop" } else { "play" }).size(12))
                    .style(button::secondary);
                if self.can_play() {
                    play = play.on_press(Message::Preview(i));
                }
                panel = panel.push(row![b, play].spacing(4).align_y(iced::Center).boxed());
            } else {
                panel = panel.push(b.boxed());
            }
        }
        panel
    }
}

impl App {
    /// The picked artwork, large: fitted to the pane or at its own size,
    /// on the show's background.
    fn preview<'a>(
        &'a self,
        session: &'a Session,
        i: usize,
        size: Size,
    ) -> Column<Element<'a, Message>> {
        const PADDING: f32 = 12.0;
        /// What a scrollbar covers, as on the stage: room past the
        /// artwork's bottom edge while it scrolls sideways.
        const SCROLLBAR: f32 = 10.0;
        let Some(asset) = self.library.get(i) else {
            return Column::new();
        };
        let mut panel = Column::new().spacing(4).padding(PADDING);
        panel = panel.push(text(&asset.name).size(16).boxed());
        let facts = match (asset.kind, asset.size) {
            (Kind::Image, Some([w, h])) => format!("image, {w} x {h} px"),
            (Kind::Vector, Some([w, h])) => format!("vector artwork, {w} x {h}"),
            (Kind::Vector, _) => "vector artwork".to_owned(),
            _ => "image".to_owned(),
        };
        panel = panel.push(text(facts).size(12).boxed());

        if let (Some(Some(thumb)), Some([w, h])) = (self.thumbs.get(i), asset.size) {
            let (w, h) = (w.max(1.0) as f32, h.max(1.0) as f32);
            let room = Size::new(
                (size.width - 2.0 * PADDING).max(1.0),
                (size.height - 2.0 * PADDING).max(1.0),
            );
            let fit = (room.width / w).min(room.height / h);
            let scale = if self.actual_size { 1.0 } else { fit };
            let (drawn_w, drawn_h) = ((w * scale).round().max(1.0), (h * scale).round().max(1.0));

            let size_button = |label: &'a str, actual: bool| {
                let mut b = button(text(label).size(13)).on_press(Message::ActualSize(actual));
                if self.actual_size == actual {
                    b = b.style(button::secondary);
                }
                b
            };
            panel = panel.push(
                container(
                    row![
                        size_button("Fit", false),
                        size_button("100%", true),
                        text(format!("{:.0}%", scale * 100.0)).size(13),
                    ]
                    .spacing(6)
                    .align_y(iced::Center),
                )
                .padding([6, 0])
                .boxed(),
            );

            let art: Element<'a, Message> = match thumb {
                Thumb::Image(handle) => image(handle.clone())
                    .width(drawn_w)
                    .height(drawn_h)
                    .content_fit(ContentFit::Fill)
                    // Enlarged pixels stay square; reduced ones blend.
                    .filter_method(if scale >= 1.0 {
                        image::FilterMethod::Nearest
                    } else {
                        image::FilterMethod::Linear
                    })
                    .boxed(),
                Thumb::Svg(handle) => svg(handle.clone())
                    .width(drawn_w)
                    .height(drawn_h)
                    .content_fit(ContentFit::Fill)
                    .boxed(),
            };
            let backdrop = lock(&session.engine)
                .show()
                .and_then(|show| cuelight_core::parse_color(&show.background))
                .map(|[r, g, b, a]| iced::Color::from_rgba8(r, g, b, f32::from(a) / 255.0))
                .unwrap_or(iced::Color::BLACK);
            let framed = container(art).style(move |_| container::Style {
                background: Some(backdrop.into()),
                ..container::Style::default()
            });
            panel = panel.push(if drawn_w > room.width {
                scrollable(container(framed).padding(iced::Padding {
                    bottom: SCROLLBAR,
                    ..iced::Padding::ZERO
                }))
                .direction(Direction::Horizontal(Scrollbar::default()))
                .width(Fill)
                .boxed()
            } else {
                framed.boxed()
            });
        }

        panel
    }
}

impl App {
    /// A picked asset: its preview or specimen, then its file, then every
    /// layer that uses it, each a link to that layer; for vector artwork,
    /// last, the element ids it is made of and which the show moves as
    /// parts.
    pub(super) fn asset_panel<'a>(
        &'a self,
        session: &'a Session,
        i: usize,
        size: Size,
    ) -> Column<Element<'a, Message>> {
        let Some(asset) = self.library.get(i) else {
            return Column::new();
        };
        let mut panel = if let Some(Some(faces)) = self.faces.get(i) {
            self.specimen_panel(session, asset, faces)
        } else if matches!(asset.kind, Kind::Image | Kind::Vector) {
            self.preview(session, i, size)
        } else {
            let engine = lock(&session.engine);
            let facts = match asset.kind {
                Kind::Sound => engine
                    .sound_duration(&asset.name)
                    .map(|d| format!("sound, {d:.2} s")),
                Kind::Video => engine
                    .video(&asset.name)
                    .map(|v| format!("video, {:.2} s, {} x {}", v.duration, v.width, v.height)),
                _ => None,
            };
            column![
                text(&asset.name).size(16),
                text(facts.unwrap_or_else(|| asset.kind.name().to_owned())).size(12)
            ]
            .spacing(4)
            .padding(12)
        };
        let heading = |label: &'a str| container(text(label).size(12)).padding([6, 0]);

        panel = panel.push(heading("FILE").boxed());
        match &asset.file {
            Some(file) => {
                panel = panel.push(text(file).size(13).boxed());
                let mut about = asset.format().unwrap_or_default();
                if let Some(bytes) = asset.bytes {
                    about = format!("{about}, {}", assets::file_size(bytes));
                }
                panel = panel.push(text(about).size(12).boxed());
            }
            None => {
                panel = panel.push(
                    text("no file: the show came without its folder")
                        .size(12)
                        .boxed(),
                )
            }
        }

        panel = panel.push(heading("USED BY").boxed());
        if asset.uses.is_empty() {
            panel = panel.push(text("nothing in this show").size(13).boxed());
            if let Some(file) = &asset.file {
                let prefix = format!("{file}: ");
                for line in session.log.audit_of(file) {
                    let said = line.text.strip_prefix(&prefix).unwrap_or(&line.text);
                    panel = panel.push(
                        text(format!("{}: {said}", line.kind.label()))
                            .size(12)
                            .boxed(),
                    );
                }
            }
        }
        for used in &asset.uses {
            panel = panel.push(
                button(column![text(&used.place).size(13), text(&used.how).size(12)].spacing(2))
                    .on_press(Message::Choose(used.path.clone()))
                    .width(Fill)
                    .padding([2, 6])
                    .style(button::text)
                    .boxed(),
            );
        }

        if let Some(structure) = &asset.structure {
            panel = panel.push(container(text("ELEMENTS").size(12)).padding([6, 0]).boxed());
            panel = panel.push(
                text(format!(
                    "{} path(s), {} inside no id",
                    structure.paths, structure.loose
                ))
                .size(12)
                .boxed(),
            );
            if structure.elements.is_empty() {
                panel = panel.push(
                    text("No element carries an id: the artwork moves only as a whole.")
                        .size(13)
                        .boxed(),
                );
            }
            for element in &structure.elements {
                let indent = element.depth as f32 * 14.0;
                panel = panel.push(
                    container(
                        row![
                            text(&element.id).size(13).width(Fill),
                            text(format!("{} path(s)", element.paths)).size(12),
                        ]
                        .spacing(6)
                        .align_y(iced::Center),
                    )
                    .padding(iced::Padding::ZERO.left(indent))
                    .boxed(),
                );
                if !element.parts.is_empty() {
                    panel = panel.push(
                        container(text(format!("part in {}", element.parts.join(", "))).size(12))
                            .padding(iced::Padding::ZERO.left(indent + 12.0))
                            .boxed(),
                    );
                }
            }
            if !structure.unknown.is_empty() {
                panel = panel.push(
                    container(text("PARTS IT DOES NOT HAVE").size(12))
                        .padding([6, 0])
                        .boxed(),
                );
                for (id, place) in &structure.unknown {
                    panel = panel.push(
                        row![
                            text(id).size(13).width(Fill),
                            text(format!("named by {place}")).size(12)
                        ]
                        .spacing(6)
                        .boxed(),
                    );
                }
            }
        }
        panel
    }
}

/// The tallest a font's sample line is drawn in its row.
const SAMPLE_HEIGHT: f32 = 48.0;

impl App {
    /// A picked font: every printable character at each size the show
    /// uses the font at, with the styles that use it so, at 100% and
    /// zoomed in.
    fn specimen_panel<'a>(
        &'a self,
        session: &'a Session,
        asset: &'a Asset,
        faces: &'a Faces,
    ) -> Column<Element<'a, Message>> {
        let engine = lock(&session.engine);
        let mut panel = Column::new().spacing(4).padding(12);
        panel = panel.push(text(&asset.name).size(16).boxed());
        let kind = match faces.sizings.first().and_then(|sizing| sizing.size) {
            None => "bitmap font",
            Some(_) => "outline font",
        };
        panel = panel.push(text(kind).size(12).boxed());
        for (sizing, lines) in faces.sizings.iter().zip(&faces.specimens) {
            panel = panel.push(space::vertical().height(8).boxed());
            panel = panel.push(text(sizing_name(sizing).to_uppercase()).size(12).boxed());
            if lines
                .iter()
                .any(|(_, _, face)| matches!(face, Face::Outline { .. }))
            {
                // The stage fills outlines through the renderer; here
                // they are iced's, in the font the file declares.
                panel = panel.push(
                    text("outlines, drawn by the editor in this font")
                        .size(12)
                        .boxed(),
                );
            }
            if sizing.styles.is_empty() {
                panel = panel.push(text("no font style uses it").size(12).boxed());
            }
            for name in &sizing.styles {
                let style = engine.show().and_then(|show| show.fonts.get(name));
                panel = panel.push(
                    row![
                        text(name).size(13),
                        text(style.map(style_name).unwrap_or_default()).size(12)
                    ]
                    .spacing(8)
                    .boxed(),
                );
            }
            // Zoomed so a line is some 24 pixels tall, for a font smaller
            // than that: pixels are looked at close.
            let tall = lines
                .iter()
                .find_map(|(_, _, face)| match face {
                    Face::Image { height, .. } => Some(*height as f32),
                    Face::Outline { size, .. } => Some(*size),
                    Face::Nothing => None,
                })
                .unwrap_or(16.0)
                .max(1.0);
            let zoom = (24.0 / tall).ceil().min(8.0);
            let zooms: &[f32] = if zoom >= 2.0 { &[1.0, zoom] } else { &[1.0] };
            for &zoom in zooms {
                let mut block = Column::<Element<'_, Message>>::new().spacing(2);
                for (code, line, face) in lines {
                    block = block.push(
                        row![
                            text(code).size(12).font(Font::MONOSPACE).width(24),
                            self.face(face, line, zoom, None)
                        ]
                        .spacing(8)
                        .align_y(iced::Center)
                        .boxed(),
                    );
                }
                panel = panel.push(text(format!("{:.0}%", zoom * 100.0)).size(12).boxed());
                panel = panel.push(
                    scrollable(block)
                        .direction(Direction::Horizontal(Scrollbar::default().spacing(4)))
                        .width(Fill)
                        .boxed(),
                );
            }
        }
        panel
    }

    /// `line` in a show's font, `zoom` times its size or at most `max`
    /// tall: the engine's pixels as they are, never smoothed while they
    /// are enlarged; an outline font's text once iced has the font.
    fn face<'a>(
        &self,
        face: &Face,
        line: &'a str,
        zoom: f32,
        max: Option<f32>,
    ) -> Element<'a, Message> {
        match face {
            Face::Image {
                handle,
                width,
                height,
            } => {
                let (width, height) = (*width as f32, *height as f32);
                let scale = max.map_or(zoom, |max| zoom.min(max / height));
                // Drawn from its left edge at its own size times the
                // scale; a narrower place cuts it off rather than
                // shrinking it.
                image(handle.clone())
                    .width(width * scale)
                    .height(height * scale)
                    .content_fit(ContentFit::None)
                    .scale(scale)
                    .filter_method(if scale >= 1.0 {
                        image::FilterMethod::Nearest
                    } else {
                        image::FilterMethod::Linear
                    })
                    .boxed()
            }
            Face::Outline { font, name, size } => {
                if !self.loaded.contains(name) {
                    return text("the font is not loaded").size(12).boxed();
                }
                let size = max.map_or(size * zoom, |max| (size * zoom).min(max));
                text(line)
                    .font(*font)
                    .size(size)
                    .wrapping(Wrapping::None)
                    .boxed()
            }
            Face::Nothing => text("none of these characters").size(12).boxed(),
        }
    }
}

/// How a font is sized: a bitmap font's own size, or pixels per em and
/// whether they are drawn as exact pixels.
fn sizing_name(sizing: &Sizing) -> String {
    match sizing.size {
        None => "bitmap, its own size".to_owned(),
        Some(size) if sizing.pixels => format!("{size} px, as pixels"),
        Some(size) => format!("{size} px"),
    }
}

/// What a font style adds to its font: colour, border and shadow.
fn style_name(style: &cuelight_core::FontStyle) -> String {
    let mut name = style.color.clone();
    if let Some(border) = &style.border {
        name.push_str(&format!(", border {} {} px", border.color, border.width));
    }
    if let Some(shadow) = &style.shadow {
        name.push_str(&format!(
            ", shadow {} at {}, {}",
            shadow.color, shadow.offset[0], shadow.offset[1]
        ));
    }
    name
}

/// How each font in a library looks, drawn by the engine once when the
/// show opens.
pub(super) fn faces(
    engine: &cuelight::Engine,
    files: &BTreeMap<String, Vec<u8>>,
    library: &[Asset],
) -> Vec<Option<Faces>> {
    library
        .iter()
        .map(|asset| {
            if asset.kind != Kind::Font {
                return None;
            }
            let show = engine.show()?;
            let outline = engine.outline_fonts().any(|(name, _)| name == asset.name);
            let look = specimen::look(files, show, &asset.name, outline);
            let face = |drawn: Option<Drawn>, sizing: &Sizing| match drawn {
                Some(Drawn::Raster(raster)) => Face::Image {
                    handle: image::Handle::from_rgba(
                        raster.width,
                        raster.height,
                        raster.pixels.to_vec(),
                    ),
                    width: raster.width,
                    height: raster.height,
                },
                Some(Drawn::Outline(Some(family))) => Face::Outline {
                    font: font(&family),
                    name: asset.name.clone(),
                    size: sizing.size.unwrap_or(16.0) as f32,
                },
                _ => Face::Nothing,
            };
            let sample = look
                .sizings
                .first()
                .map_or(Face::Nothing, |sizing| face(look.sample, sizing));
            let specimens = look
                .specimens
                .into_iter()
                .zip(&look.sizings)
                .map(|(lines, sizing)| {
                    specimen::rows()
                        .into_iter()
                        .zip(lines)
                        .map(|((code, _), line)| {
                            let drawn = face(line.drawn, sizing);
                            (code, line.text, drawn)
                        })
                        .collect()
                })
                .collect();
            Some(Faces {
                sizings: look.sizings,
                sample,
                specimens,
            })
        })
        .collect()
}

/// The iced font for an outline font's face: its family at the
/// nearest of iced's weights, italic when it slants.
fn font(family: &specimen::Family) -> Font {
    use iced::font::{Style, Weight};
    const WEIGHTS: [Weight; 9] = [
        Weight::Thin,
        Weight::ExtraLight,
        Weight::Light,
        Weight::Normal,
        Weight::Medium,
        Weight::Semibold,
        Weight::Bold,
        Weight::ExtraBold,
        Weight::Black,
    ];
    let step = (usize::from(family.weight.clamp(100, 900)) + 50) / 100 - 1;
    Font::with_family(family.name.as_str())
        .weight(WEIGHTS.get(step).copied().unwrap_or(Weight::Normal))
        .style(if family.italic {
            Style::Italic
        } else {
            Style::Normal
        })
}

/// Give iced the show's outline fonts, so their samples can be drawn in
/// them.
pub(super) fn load_fonts(engine: &cuelight::Engine, library: &[Asset]) -> Task<Message> {
    let loads: Vec<Task<Message>> = engine
        .outline_fonts()
        .filter(|(name, _)| {
            library
                .iter()
                .any(|asset| asset.kind == Kind::Font && asset.name == *name)
        })
        .map(|(name, bytes)| {
            let name = name.to_owned();
            iced::font::load(bytes.to_vec()).map(move |result| {
                if let Err(error) = &result {
                    log::warn!("font {name}: {error:?}");
                }
                Message::FontLoaded(name.clone(), result.is_ok())
            })
        })
        .collect();
    Task::batch(loads)
}

/// Thumbnails for the artwork in a library: an image's pixels as the
/// engine decoded them, an SVG's bytes as the show shipped them.
pub(super) fn thumbs(engine: &cuelight::Engine, library: &[Asset]) -> Vec<Option<Thumb>> {
    library
        .iter()
        .map(|asset| match asset.kind {
            Kind::Image => engine.image(&asset.name).map(|i| {
                Thumb::Image(image::Handle::from_rgba(
                    i.width,
                    i.height,
                    i.pixels.to_vec(),
                ))
            }),
            Kind::Vector => asset
                .svg
                .as_ref()
                .map(|bytes| Thumb::Svg(svg::Handle::from_memory(bytes.to_vec()))),
            _ => None,
        })
        .collect()
}

/// A stand-in for a thumbnail, for the kinds that have none.
fn kind_mark(kind: Kind) -> &'static str {
    match kind {
        Kind::Image | Kind::Vector => "?",
        Kind::Font => "Aa",
        Kind::Sound => "))",
        Kind::Video => ">",
    }
}
