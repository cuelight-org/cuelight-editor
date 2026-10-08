//! The library area: the layer tree, or the assets.

use cuelight_editor_core::tree::Row;
use iced::widget::Widget as _;
use iced::widget::operation::Animation;
use iced::widget::operation::scrollable::scroll_to;
use iced::widget::scrollable::AbsoluteOffset;
use iced::widget::{Column, button, column, container, row, scrollable, space, text};
use iced::{Element, Fill, Task};

use super::{App, Message, Tab};
use cuelight_editor_core::session::Session;

impl App {
    /// The library area: a tab row over the show's layers or its assets.
    pub(super) fn library_panel<'a>(&'a self, session: &'a Session) -> Element<'a, Message> {
        let tab = |label: &'a str, tab: Tab| {
            let mut b = button(text(label).size(13)).on_press(Message::Tab(tab));
            if self.tab == tab {
                b = b.style(button::secondary);
            }
            b
        };
        let tabs = row![tab("Layers", Tab::Layers), tab("Assets", Tab::Assets)].spacing(6);
        let body = match self.tab {
            Tab::Layers => self.tree_panel(session),
            Tab::Assets => self.assets_panel(),
        };
        // The tree has its menu and buttons above it, out of its scroll.
        let bar = (self.tab == Tab::Layers).then(|| container(self.arrange_bar()).padding([4, 12]));
        column![
            container(tabs).padding([4, 8]),
            bar,
            scrollable(body)
                .id(TREE)
                .on_scroll(|scroll| {
                    Message::TreeScrolled(
                        scroll.viewport.absolute_offset().y,
                        scroll.viewport.bounds.height,
                    )
                })
                .width(Fill)
                .height(Fill)
        ]
        .width(Fill)
        .height(Fill)
        .boxed()
    }

    /// The layers as a tree: the show's own, then each scene's, groups'
    /// children indented under them; the picked ones marked (a scene's
    /// heading while it is picked, the show's while nothing is), the
    /// active scene starred.
    fn tree_panel<'a>(&'a self, session: &'a Session) -> Column<Element<'a, Message>> {
        let mut panel = Column::new().spacing(2).padding(12);
        let active = session.active_scene();
        for row_ in &self.rows {
            match row_ {
                Row::Root {
                    root: cuelight_core::Root::Show,
                    ..
                } => {
                    // The show's heading picks the show itself: its
                    // settings in the inspector.
                    let mut b = button(text("SHOW").size(12))
                        .on_press(Message::Deselect)
                        .width(Fill)
                        .padding([6, 0])
                        .style(button::text);
                    if self.selection.is_empty() && self.scene.is_none() {
                        b = b.style(button::secondary);
                    }
                    panel = panel.push(b.boxed());
                }
                // A scene's heading picks the scene and enters it.
                Row::Root {
                    root: cuelight_core::Root::Scene(i),
                    name,
                } => {
                    let heading = if active.as_deref() == Some(name) {
                        format!("SCENE {name} *")
                    } else {
                        format!("SCENE {name}")
                    };
                    let mut b = button(text(heading).size(12))
                        .on_press(Message::PickScene(*i))
                        .width(Fill)
                        .padding([6, 0])
                        .style(button::text);
                    if self.selection.is_empty() && self.scene == Some(*i) {
                        b = b.style(button::secondary);
                    }
                    panel = panel.push(b.boxed());
                }
                Row::Layer {
                    path,
                    name,
                    kind,
                    depth,
                } => {
                    let line = row![
                        space::horizontal().width(*depth as f32 * 14.0),
                        text(*kind).size(11).width(44),
                        text(name).size(14),
                    ]
                    .spacing(6)
                    .align_y(iced::Center);
                    let mut b = button(line)
                        .on_press(Message::Choose(path.clone()))
                        .width(Fill)
                        .padding([2, 6])
                        .style(button::text);
                    if self.selection.contains(path) {
                        b = b.style(button::secondary);
                    }
                    panel = panel.push(b.boxed());
                }
            }
        }
        panel
    }

    /// Scroll the tree just far enough that the row of what is picked
    /// (the last layer, or the scene) is in view. Every row has a set
    /// height, so where one is is added up rather than measured.
    pub(super) fn reveal_in_tree(&self) -> Task<Message> {
        // A heading: 12 px text, 6 above and below; a layer: 14 px
        // text, 2 above and below; 2 between rows, 12 round the list.
        const HEADING: f32 = 12.0 * 1.3 + 12.0;
        const LAYER: f32 = 14.0 * 1.3 + 4.0;
        const GAP: f32 = 2.0;
        let picked = |row: &Row| match row {
            Row::Layer { path, .. } => self.selection.last() == Some(path),
            Row::Root {
                root: cuelight_core::Root::Scene(i),
                ..
            } => self.selection.is_empty() && self.scene == Some(*i),
            _ => false,
        };
        let mut top = 12.0;
        let mut found = None;
        for row in &self.rows {
            let height = match row {
                Row::Root { .. } => HEADING,
                Row::Layer { .. } => LAYER,
            };
            if picked(row) {
                found = Some((top, height));
                break;
            }
            top += height + GAP;
        }
        let Some((top, height)) = found else {
            return Task::none();
        };
        let (offset, view) = self.tree_view;
        // Before the first scroll the view's height is not known: half
        // a window is a safe guess.
        let view = if view > 0.0 { view } else { 300.0 };
        let to = if top < offset {
            top - 12.0
        } else if top + height > offset + view {
            top + height - view + 12.0
        } else {
            return Task::none();
        };
        scroll_to(
            TREE,
            AbsoluteOffset {
                x: 0.0,
                y: to.max(0.0),
            },
            Animation::Instant,
        )
    }
}

/// The tree's scroll pane, for bringing a row into view.
const TREE: &str = "tree";
