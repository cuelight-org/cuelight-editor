//! The stage: the show drawn at its zoom in a scroll pane, the log
//! under it, and what a click on it picks.

use cuelight_core::LayerPath;
use iced::widget::operation::{Animation, scroll_to, snap_to};
use iced::widget::scrollable::{AbsoluteOffset, Direction, RelativeOffset, Scrollbar};
use iced::widget::{Column, button, column, container, row, scrollable, shader, space, text};
use iced::{Element, Fill, Size, Task};

use super::{App, Message, Zoom};
use crate::stage::{Pick, Stage};
use cuelight_editor_core::session::Session;

/// The stage's scroll pane, for the tasks that position it.
const STAGE: &str = "stage";

impl App {
    /// The stage area: a zoom bar over the show drawn at its scale, in a
    /// scroll pane with room round it of half the view on every side, so
    /// the show can be scrolled until its edge sits in the middle of the
    /// view, the way drawing tools have it.
    pub(super) fn stage<'a>(&'a self, session: &'a Session, size: Size) -> Element<'a, Message> {
        const BAR: f32 = 36.0;
        const MARGIN: f32 = 8.0;
        let [show_w, show_h] = self.summary.size.map(|n| n.max(1) as f32);
        let room = Size::new(
            (size.width - 2.0 * MARGIN).max(1.0),
            (size.height - BAR - 2.0 * MARGIN).max(1.0),
        );
        let fit = (room.width / show_w).min(room.height / show_h);
        self.fitted.set(fit);
        let scale = match self.zoom {
            Zoom::Fit => fit,
            Zoom::Scale(scale) => scale,
        };
        let (w, h) = ((show_w * scale).round(), (show_h * scale).round());
        // The room round the show: half the view on every side, so the
        // pane's bars stand for show plus room and its own background
        // shows round the show; the scrollbars float over room, never
        // over the show's far edge.
        let around = iced::Padding {
            top: (room.height / 2.0).round(),
            bottom: (room.height / 2.0).round(),
            left: (room.width / 2.0).round(),
            right: (room.width / 2.0).round(),
        };

        let zoom_button = |label: &'a str, zoom: Zoom| {
            let mut b = button(text(label).size(13)).on_press(Message::Zoom(zoom));
            if self.zoom == zoom {
                b = b.style(button::secondary);
            }
            b
        };
        let bar = row![
            zoom_button("Fit", Zoom::Fit),
            zoom_button("100%", Zoom::Scale(1.0)),
            button(text("-").size(13)).on_press(Message::ZoomBy(1.0 / Zoom::STEP)),
            button(text("+").size(13)).on_press(Message::ZoomBy(Zoom::STEP)),
            text(format!("{:.0}%", scale * 100.0)).size(13),
        ]
        .spacing(6)
        .align_y(iced::Center);

        let stage = shader(Stage {
            engine: session.engine.clone(),
            revision: session.revision,
            selection: self.selection.clone(),
            on_pick: Message::Pick,
            on_press: Message::Press,
        })
        .width(w)
        .height(h);
        // The pane fills the room, with the margin outside it, and is
        // positioned by the tasks that centre it and keep the middle on
        // a zoom.
        let scrolled = scrollable(container(stage).padding(around))
            .id(STAGE)
            .width(Fill)
            .height(Fill)
            .on_scroll(|scroll| Message::Scrolled(scroll.viewport.absolute_offset()))
            .direction(Direction::Both {
                vertical: Scrollbar::default(),
                horizontal: Scrollbar::default(),
            })
            // The corner where the two scrollbars meet is the window's,
            // not the show's.
            .style(|theme, status| {
                let mut style = scrollable::default(theme, status);
                style.gap = Some(theme.palette().background.weak.color.into());
                style
            });
        column![
            container(bar).padding([4, 8]).height(BAR),
            container(scrolled).padding(MARGIN).width(Fill).height(Fill)
        ]
        .width(Fill)
        .height(Fill)
        .into()
    }

    /// The log under the stage: a header saying how many lines, which
    /// folds it to itself; unfolded, the lines follow the newest.
    pub(super) fn log_panel<'a>(&'a self, session: &'a Session) -> Element<'a, Message> {
        const HEIGHT: f32 = 160.0;
        let count = session.log.len();
        let header = row![
            text("LOG").size(12),
            text(format!("{count} line(s)")).size(12),
            space::horizontal(),
            button(text(if self.log_open { "fold" } else { "unfold" }).size(12))
                .on_press(Message::ToggleLog)
                .style(button::text),
        ]
        .spacing(12)
        .align_y(iced::Center);
        let mut panel = column![container(header).padding([0, 8]).width(Fill)].spacing(4);
        if self.log_open {
            let mut lines = Column::new().spacing(1).padding([0, 8]);
            for line in session.log.lines() {
                lines = lines.push(
                    text(line.render())
                        .size(12)
                        .font(iced::Font::new("DM Mono"))
                        .wrapping(text::Wrapping::None),
                );
            }
            panel = panel.push(
                scrollable(lines)
                    .direction(Direction::Both {
                        vertical: Scrollbar::default(),
                        horizontal: Scrollbar::default(),
                    })
                    .anchor_bottom()
                    .width(Fill)
                    .height(HEIGHT),
            );
        }
        container(panel).padding([4, 0]).width(Fill).into()
    }
}

impl App {
    /// What a click on the stage does to the selection, given the layers
    /// under it, topmost first: a plain click takes the topmost, Alt the
    /// next one down from the one picked last, Shift adds or removes
    /// rather than replaces; a click on nothing clears.
    pub(super) fn pick(&mut self, under: Vec<LayerPath>, pick: Pick) {
        self.expanded = None;
        let Some(first) = under.first() else {
            if !pick.shift {
                self.selection.clear();
            }
            return;
        };
        let chosen = if pick.alt {
            let last = self.selection.last();
            let at = last.and_then(|last| under.iter().position(|p| p == last));
            at.and_then(|i| under.get((i + 1) % under.len()))
                .unwrap_or(first)
        } else {
            first
        };
        if pick.shift {
            match self.selection.iter().position(|p| p == chosen) {
                Some(i) => {
                    self.selection.remove(i);
                }
                None => self.selection.push(chosen.clone()),
            }
        } else {
            self.selection = vec![chosen.clone()];
        }
    }

    /// The scale the stage draws at now.
    pub(super) fn scale(&self) -> f32 {
        match self.zoom {
            Zoom::Fit => self.fitted.get(),
            Zoom::Scale(scale) => scale,
        }
    }

    /// A zoom to `scale`, kept between the smallest and the largest the
    /// stage can draw: vello stops past a frame of about 4096 x 4096
    /// physical pixels, whatever the show's size.
    pub(super) fn zoom_to(&self, scale: f32) -> Zoom {
        Zoom::Scale(scale.clamp(Zoom::MIN, self.max_zoom()))
    }

    /// Put the show in the middle of the stage: what a fresh open and a
    /// fit do.
    pub(super) fn centre_stage(&mut self) -> Task<Message> {
        self.scrolled = None;
        snap_to(STAGE, RelativeOffset { x: 0.5, y: 0.5 }, Animation::Instant)
    }

    /// After a zoom from the scale `before`, keep the canvas point that was
    /// under the middle of the view there. The room round the show is half
    /// the view on every side, so that point is the scroll offset over the
    /// scale, and the new offset is the old one scaled.
    pub(super) fn keep_middle(&mut self, before: f32) -> Task<Message> {
        let Some(offset) = self.scrolled else {
            return self.centre_stage();
        };
        let ratio = self.scale() / before.max(f32::EPSILON);
        let to = AbsoluteOffset {
            x: offset.x * ratio,
            y: offset.y * ratio,
        };
        self.scrolled = Some(to);
        scroll_to(STAGE, to, Animation::Instant)
    }

    /// The largest zoom whose frame vello still draws.
    fn max_zoom(&self) -> f32 {
        let [w, h] = self.summary.size.map(|n| n.max(1) as f32);
        let frame = |scale: f32| {
            [
                (w * scale * self.scale_factor).round() as u32,
                (h * scale * self.scale_factor).round() as u32,
            ]
        };
        let mut scale = Zoom::MAX;
        while scale > Zoom::MIN && !crate::stage::drawable(frame(scale)) {
            scale /= 1.02;
        }
        scale.max(Zoom::MIN)
    }
}
