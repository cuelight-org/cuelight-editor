//! The library area: the layer tree, or the assets.

use cuelight_editor_core::tree::Row;
use iced::widget::Widget as _;
use iced::widget::{Column, button, column, container, row, scrollable, space, text};
use iced::{Element, Fill};

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
            scrollable(body).width(Fill).height(Fill)
        ]
        .width(Fill)
        .height(Fill)
        .boxed()
    }

    /// The layers as a tree: the show's own, then each scene's, groups'
    /// children indented under them; the picked ones marked (the show
    /// while nothing is), the active scene starred.
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
                    if self.selection.is_empty() {
                        b = b.style(button::secondary);
                    }
                    panel = panel.push(b.boxed());
                }
                // A scene's heading enters the scene.
                Row::Root {
                    root: cuelight_core::Root::Scene(i),
                    name,
                } => {
                    let heading = if active.as_deref() == Some(name) {
                        format!("SCENE {name} *")
                    } else {
                        format!("SCENE {name}")
                    };
                    panel = panel.push(
                        button(text(heading).size(12))
                            .on_press(Message::EnterScene(*i))
                            .width(Fill)
                            .padding([6, 0])
                            .style(button::text)
                            .boxed(),
                    );
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
}
