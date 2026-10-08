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
        let lines = self.tree_lines();
        for (row_, line) in self.rows.iter().zip(&lines) {
            if line.hidden {
                continue;
            }
            let fold = self.fold_toggle(line);
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
                    panel = panel.push(row![fold, b].align_y(iced::Center).height(HEADING).boxed());
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
                    panel = panel.push(row![fold, b].align_y(iced::Center).height(HEADING).boxed());
                }
                Row::Layer {
                    path,
                    name,
                    kind,
                    depth,
                } => {
                    let line = row![text(*kind).size(11).width(44), text(name).size(14),]
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
                    panel = panel.push(
                        row![space::horizontal().width(*depth as f32 * 14.0), fold, b]
                            .align_y(iced::Center)
                            .height(LAYER)
                            .boxed(),
                    );
                }
            }
        }
        panel
    }

    /// Scroll the tree just far enough that the row of what is picked
    /// (the last layer, or the scene) is in view. Every row has a set
    /// height, so where one is is added up rather than measured.
    pub(super) fn reveal_in_tree(&self) -> Task<Message> {
        // 2 between rows, 12 round the list.
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
        let lines = self.tree_lines();
        for (row, line) in self.rows.iter().zip(&lines) {
            if line.hidden {
                continue;
            }
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
        // Out of view, the row goes to the top of it. Until the tree has
        // scrolled once its view's height is not known, and the row is
        // taken to be out of view.
        let (offset, view) = self.tree_view;
        if top >= offset && view > 0.0 && top + height <= offset + view {
            return Task::none();
        }
        let to = top - 12.0;
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

/// How tall a heading's row is in the tree, and a layer's: set, so
/// where a row is can be added up.
const HEADING: f32 = 30.0;
const LAYER: f32 = 25.0;

/// A row of the tree as folding sees it.
pub(super) struct TreeLine {
    /// What the row is folded by: its heading's or group's names, which
    /// a reorder does not change.
    pub key: String,
    /// Under a heading or a group that is folded.
    pub hidden: bool,
    /// Rows sit under it, so it can fold.
    pub parent: bool,
    /// Folded, with something picked inside it.
    pub holds_picked: bool,
}

impl App {
    /// Every row's fold key, whether it is folded away and whether it
    /// folds, in the order of the rows.
    pub(super) fn tree_lines(&self) -> Vec<TreeLine> {
        let mut out: Vec<TreeLine> = Vec::with_capacity(self.rows.len());
        // The heading's key, then each enclosing layer's, by depth.
        let mut root = String::new();
        let mut above: Vec<String> = Vec::new();
        for (i, row) in self.rows.iter().enumerate() {
            let next = self.rows.get(i + 1);
            let (key, hidden, parent) = match row {
                Row::Root { root: r, name } => {
                    root = match r {
                        cuelight_core::Root::Show => "show".to_owned(),
                        cuelight_core::Root::Scene(_) => format!("scene {name}"),
                    };
                    above.clear();
                    (root.clone(), false, matches!(next, Some(Row::Layer { .. })))
                }
                Row::Layer { name, depth, .. } => {
                    above.truncate(*depth);
                    let hidden = self.folded.contains(&root)
                        || above.iter().any(|key| self.folded.contains(key));
                    let key = format!("{}/{name}", above.last().unwrap_or(&root));
                    let parent = matches!(next, Some(Row::Layer { depth: d, .. }) if d > depth);
                    above.push(key.clone());
                    (key, hidden, parent)
                }
            };
            out.push(TreeLine {
                key,
                hidden,
                parent,
                holds_picked: false,
            });
        }
        // A picked layer folded away marks the row that hides it: the
        // first one up from it that shows.
        for (i, row) in self.rows.iter().enumerate() {
            let Row::Layer { path, depth, .. } = row else {
                continue;
            };
            if !self.selection.contains(path) || !out.get(i).is_some_and(|l| l.hidden) {
                continue;
            }
            let holder = (0..i).rev().find(|j| {
                let shows = out.get(*j).is_some_and(|l| !l.hidden);
                let above = match self.rows.get(*j) {
                    Some(Row::Layer { depth: d, .. }) => d < depth,
                    Some(Row::Root { .. }) => true,
                    None => false,
                };
                shows && above
            });
            if let Some(line) = holder.and_then(|j| out.get_mut(j)) {
                line.holds_picked = true;
            }
        }
        out
    }

    /// The toggle that folds a row, or room for one where nothing sits
    /// under it.
    fn fold_toggle<'a>(&self, line: &TreeLine) -> Element<'a, Message> {
        if !line.parent {
            return space::horizontal().width(16).boxed();
        }
        let folded = self.folded.contains(&line.key);
        let holds = line.holds_picked;
        button(text(if folded { "+" } else { "-" }).size(12))
            .on_press(Message::ToggleFold(line.key.clone()))
            .width(16)
            .padding([2, 0])
            .style(move |theme: &iced::Theme, status| {
                let mut style = button::text(theme, status);
                // What is picked is folded away in here.
                if holds {
                    style.text_color = theme.palette().primary.base.color;
                }
                style
            })
            .boxed()
    }

    /// Unfold whatever hides the picked layer or scene, so the tree
    /// shows it.
    pub(super) fn unfold_to_picked(&mut self) {
        let lines = self.tree_lines();
        let Some(at) = self.rows.iter().position(|row| match row {
            Row::Layer { path, .. } => self.selection.last() == Some(path),
            _ => false,
        }) else {
            return;
        };
        // Every row above it in the tree that holds it.
        let Some(Row::Layer { depth, .. }) = self.rows.get(at) else {
            return;
        };
        let mut depth = *depth;
        for (row, line) in self.rows.iter().zip(&lines).take(at).rev() {
            match row {
                Row::Layer { depth: d, .. } if *d < depth => {
                    self.folded.remove(&line.key);
                    depth = *d;
                }
                Row::Root { .. } => {
                    self.folded.remove(&line.key);
                    break;
                }
                _ => {}
            }
        }
    }
}
