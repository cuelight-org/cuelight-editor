//! The show document as the editor edits it: a JSON tree that remembers
//! the text every node was read from, so a save writes untouched nodes
//! back byte for byte and an edit changes only what it touched.
//!
//! An edit is made on a JSON pointer and carries what it replaced, so
//! its inverse is the same edit the other way round: undo and redo are
//! two stacks of edits, and undoing everything gives the text back
//! exactly, `1.0` where `1.0` was written and the keys in their order.

use std::fmt;

use serde_json::Value;

/// A step of a JSON pointer: into an object by key, into an array by
/// index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Key(String),
    Index(usize),
}

/// Where in the document: a JSON pointer (RFC 6901), `/layers/2/x`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Pointer(pub Vec<Step>);

impl Pointer {
    pub fn parse(text: &str) -> Result<Self, EditError> {
        if text.is_empty() {
            return Ok(Self::default());
        }
        let Some(rest) = text.strip_prefix('/') else {
            return Err(EditError::BadPointer(text.to_owned()));
        };
        Ok(Self(
            rest.split('/')
                .map(|part| {
                    let part = part.replace("~1", "/").replace("~0", "~");
                    match part.parse::<usize>() {
                        Ok(i) if part.len() == 1 || !part.starts_with('0') => Step::Index(i),
                        _ => Step::Key(part),
                    }
                })
                .collect(),
        ))
    }

    /// This pointer one step further down.
    pub fn then(&self, step: Step) -> Self {
        let mut steps = self.0.clone();
        steps.push(step);
        Self(steps)
    }

    /// The parent and the last step, or `None` at the root.
    fn split(&self) -> Option<(Pointer, &Step)> {
        let (last, parent) = self.0.split_last()?;
        Some((Pointer(parent.to_vec()), last))
    }
}

impl fmt::Display for Pointer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for step in &self.0 {
            match step {
                Step::Key(key) => write!(f, "/{}", key.replace('~', "~0").replace('/', "~1"))?,
                Step::Index(i) => write!(f, "/{i}")?,
            }
        }
        Ok(())
    }
}

/// One item of an array or object: the text before it (whitespace, and
/// the comma from the item before), its key as written for an object,
/// and its node.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// Everything between the previous node (or the opening bracket) and
    /// this one: `",\n    "`.
    pub before: String,
    /// The key with its quotes and the colon, `"x": `, or empty in an
    /// array.
    pub key: String,
    pub node: Node,
}

/// A node of the tree.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    /// A scalar as written, `1.0` or `"abc"`, and what it means.
    Scalar { raw: String, value: Value },
    /// `[` items `]`, with what stands between the last item and `]`.
    Array { items: Vec<Item>, tail: String },
    /// `{` items `}`, likewise.
    Object { items: Vec<Item>, tail: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum EditError {
    BadPointer(String),
    /// The pointer leads nowhere.
    NotFound(Pointer),
    /// The pointer's parent is not the kind the edit needs.
    NotAContainer(Pointer),
    /// An index past the end of the array.
    OutOfRange(Pointer),
    /// A key the object already has.
    Exists(Pointer),
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EditError::BadPointer(text) => write!(f, "not a JSON pointer: {text:?}"),
            EditError::NotFound(path) => write!(f, "nothing at {path}"),
            EditError::NotAContainer(path) => write!(f, "{path} holds no items"),
            EditError::OutOfRange(path) => write!(f, "{path} is past the end"),
            EditError::Exists(path) => write!(f, "{path} is already there"),
        }
    }
}

impl std::error::Error for EditError {}

/// A parse error: where in the text, and what was expected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub at: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "at byte {}: {}", self.at, self.message)
    }
}

impl std::error::Error for ParseError {}

/// One edit, with everything undo needs.
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    /// The node at `path` replaced.
    Set {
        path: Pointer,
        before: Node,
        after: Node,
    },
    /// An item put into the array or object at `path`'s parent, at
    /// `index` among its items (the key `path` ends with, for an
    /// object). `shifted` is the text the item after it has before it
    /// while this one is in: the separator the insert gave it, since it
    /// took the inserted item's own.
    Insert {
        path: Pointer,
        index: usize,
        item: Item,
        shifted: Option<String>,
    },
    /// The item at `path` taken out, from `index`; `shifted` is what the
    /// item after it had before it while this one was in.
    Remove {
        path: Pointer,
        index: usize,
        item: Item,
        shifted: Option<String>,
    },
}

impl Edit {
    /// The same edit the other way round.
    fn inverse(self) -> Edit {
        match self {
            Edit::Set {
                path,
                before,
                after,
            } => Edit::Set {
                path,
                before: after,
                after: before,
            },
            Edit::Insert {
                path,
                index,
                item,
                shifted,
            } => Edit::Remove {
                path,
                index,
                item,
                shifted,
            },
            Edit::Remove {
                path,
                index,
                item,
                shifted,
            } => Edit::Insert {
                path,
                index,
                item,
                shifted,
            },
        }
    }

    pub fn path(&self) -> &Pointer {
        match self {
            Edit::Set { path, .. } | Edit::Insert { path, .. } | Edit::Remove { path, .. } => path,
        }
    }
}

/// The document: its tree, and the edits made to it.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// Text before the root node and after it.
    head: String,
    root: Node,
    foot: String,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    /// How far down the undo stack the saved state lies, or `None` when
    /// it was undone past.
    saved: Option<usize>,
}

impl Document {
    /// Read a document, keeping every node's text.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let mut parser = Parser { text, at: 0 };
        let head = parser.whitespace();
        let root = parser.node()?;
        let foot = parser.whitespace();
        if parser.at != text.len() {
            return Err(parser.error("text after the document"));
        }
        Ok(Self {
            head,
            root,
            foot,
            undo: Vec::new(),
            redo: Vec::new(),
            saved: Some(0),
        })
    }

    /// The document as text: untouched nodes exactly as read.
    pub fn text(&self) -> String {
        let mut out = String::with_capacity(4096);
        out.push_str(&self.head);
        self.root.write(&mut out);
        out.push_str(&self.foot);
        out
    }

    /// The document as a value, for typed reads.
    pub fn value(&self) -> Value {
        self.root.value()
    }

    pub fn root(&self) -> &Node {
        &self.root
    }

    pub fn get(&self, path: &Pointer) -> Option<&Node> {
        self.root.get(&path.0)
    }

    /// Replace what is at `path` with `value`.
    pub fn set(&mut self, path: &Pointer, value: Value) -> Result<(), EditError> {
        let indent = self.indent_at(path);
        let after = Node::from_value(&value, &indent);
        let target = self
            .root
            .get_mut(&path.0)
            .ok_or_else(|| EditError::NotFound(path.clone()))?;
        let before = std::mem::replace(target, after.clone());
        self.record(Edit::Set {
            path: path.clone(),
            before,
            after,
        });
        Ok(())
    }

    /// Put `value` into the array or object `path`'s parent names, at
    /// the index or under the key `path` ends with. An index equal to
    /// the array's length appends.
    pub fn insert(&mut self, path: &Pointer, value: Value) -> Result<(), EditError> {
        let (parent, step) = path
            .split()
            .ok_or_else(|| EditError::NotAContainer(path.clone()))?;
        let indent = self.indent_in(&parent);
        let node = Node::from_value(&value, &indent);
        let container = self
            .root
            .get_mut(&parent.0)
            .ok_or_else(|| EditError::NotFound(parent.clone()))?;
        let key = match (&*container, step) {
            (Node::Object { .. }, Step::Key(key)) => format!("{}: ", Value::String(key.clone())),
            (Node::Array { .. }, Step::Index(_)) => String::new(),
            _ => return Err(EditError::NotAContainer(path.clone())),
        };
        let item = Item {
            before: String::new(),
            key,
            node,
        };
        let (index, item, shifted) = container.insert(step, item, path)?;
        self.record(Edit::Insert {
            path: path.clone(),
            index,
            item,
            shifted,
        });
        Ok(())
    }

    /// Take out what is at `path`.
    pub fn remove(&mut self, path: &Pointer) -> Result<(), EditError> {
        let (parent, step) = path
            .split()
            .ok_or_else(|| EditError::NotAContainer(path.clone()))?;
        let container = self
            .root
            .get_mut(&parent.0)
            .ok_or_else(|| EditError::NotFound(parent.clone()))?;
        let index = container
            .index_of(step)
            .ok_or_else(|| EditError::NotFound(path.clone()))?;
        let (item, shifted) = container.take(index);
        self.record(Edit::Remove {
            path: path.clone(),
            index,
            item,
            shifted,
        });
        Ok(())
    }

    fn record(&mut self, edit: Edit) {
        self.undo.push(edit);
        self.redo.clear();
        if self.saved.is_some_and(|depth| depth >= self.undo.len()) {
            self.saved = None;
        }
    }

    /// Take the last edit back. `false` when there is none.
    pub fn undo(&mut self) -> bool {
        let Some(edit) = self.undo.pop() else {
            return false;
        };
        let inverse = edit.inverse();
        self.apply(&inverse);
        self.redo.push(inverse.inverse());
        true
    }

    /// Make the last undone edit again. `false` when there is none.
    pub fn redo(&mut self) -> bool {
        let Some(edit) = self.redo.pop() else {
            return false;
        };
        self.apply(&edit);
        self.undo.push(edit);
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The edits made since the document was read or last saved.
    pub fn edits(&self) -> &[Edit] {
        &self.undo
    }

    /// Whether the document differs from what was last saved.
    pub fn is_dirty(&self) -> bool {
        self.saved != Some(self.undo.len())
    }

    /// The document as it stands is what is on disk now.
    pub fn mark_saved(&mut self) {
        self.saved = Some(self.undo.len());
    }

    /// Apply an edit whose inverse was recorded already; the tree is
    /// known to fit it.
    fn apply(&mut self, edit: &Edit) {
        match edit {
            Edit::Set { path, after, .. } => {
                if let Some(target) = self.root.get_mut(&path.0) {
                    *target = after.clone();
                }
            }
            Edit::Insert {
                path,
                index,
                item,
                shifted,
            } => {
                if let Some((parent, _)) = path.split()
                    && let Some(container) = self.root.get_mut(&parent.0)
                {
                    container.put(*index, item.clone(), shifted.clone());
                }
            }
            Edit::Remove { path, index, .. } => {
                if let Some((parent, _)) = path.split()
                    && let Some(container) = self.root.get_mut(&parent.0)
                {
                    container.take(*index);
                }
            }
        }
    }

    /// The indentation of the node at `path`: the whitespace after the
    /// last newline before it, for a new node written in its place.
    fn indent_at(&self, path: &Pointer) -> String {
        match path.split() {
            Some((parent, step)) => match self.root.get(&parent.0) {
                Some(container) => container
                    .item(step)
                    .map(|item| indent_of(&item.before))
                    .unwrap_or_default(),
                None => String::new(),
            },
            None => String::new(),
        }
    }

    /// The indentation of the items in the container at `path`.
    fn indent_in(&self, path: &Pointer) -> String {
        match self.root.get(&path.0) {
            Some(Node::Array { items, .. } | Node::Object { items, .. }) => items
                .iter()
                .find_map(|item| item.before.contains('\n').then(|| indent_of(&item.before)))
                .unwrap_or_default(),
            _ => String::new(),
        }
    }
}

/// The whitespace after the last newline of `gap`; empty without one.
fn indent_of(gap: &str) -> String {
    match gap.rfind('\n') {
        Some(i) => gap[i + 1..].to_owned(),
        None => String::new(),
    }
}

impl Node {
    fn get(&self, path: &[Step]) -> Option<&Node> {
        let Some((step, rest)) = path.split_first() else {
            return Some(self);
        };
        self.item(step)?.node.get(rest)
    }

    fn get_mut(&mut self, path: &[Step]) -> Option<&mut Node> {
        let Some((step, rest)) = path.split_first() else {
            return Some(self);
        };
        self.item_mut(step)?.node.get_mut(rest)
    }

    fn item(&self, step: &Step) -> Option<&Item> {
        match (self, step) {
            (Node::Array { items, .. }, Step::Index(i)) => items.get(*i),
            (Node::Object { items, .. }, Step::Key(key)) => items
                .iter()
                .find(|item| item.key_name().as_deref() == Some(key)),
            _ => None,
        }
    }

    fn item_mut(&mut self, step: &Step) -> Option<&mut Item> {
        match (self, step) {
            (Node::Array { items, .. }, Step::Index(i)) => items.get_mut(*i),
            (Node::Object { items, .. }, Step::Key(key)) => items
                .iter_mut()
                .find(|item| item.key_name().as_deref() == Some(key)),
            _ => None,
        }
    }

    /// Where the item `step` names sits among the items.
    fn index_of(&self, step: &Step) -> Option<usize> {
        match (self, step) {
            (Node::Array { items, .. }, Step::Index(i)) => (*i < items.len()).then_some(*i),
            (Node::Object { items, .. }, Step::Key(key)) => items
                .iter()
                .position(|it| it.key_name().as_deref() == Some(key)),
            _ => None,
        }
    }

    /// Insert `item` where `step` says: at an index of an array (its
    /// length appends), or at the end of an object under a key it does
    /// not have yet. The new item takes the text the item there had
    /// before it, and that one gets a separator.
    fn insert(
        &mut self,
        step: &Step,
        item: Item,
        path: &Pointer,
    ) -> Result<(usize, Item, Option<String>), EditError> {
        let index = match (&*self, step) {
            (Node::Array { items, .. }, Step::Index(i)) => {
                if *i > items.len() {
                    return Err(EditError::OutOfRange(path.clone()));
                }
                *i
            }
            (Node::Object { items, .. }, Step::Key(key)) => {
                if items.iter().any(|it| it.key_name().as_deref() == Some(key)) {
                    return Err(EditError::Exists(path.clone()));
                }
                items.len()
            }
            _ => return Err(EditError::NotAContainer(path.clone())),
        };
        let separator = self.separator();
        let (Node::Array { items, .. } | Node::Object { items, .. }) = self else {
            unreachable!("checked above");
        };
        let mut item = item;
        let shifted = if index < items.len() {
            item.before = std::mem::replace(&mut items[index].before, separator.clone());
            Some(separator)
        } else {
            item.before = if items.is_empty() {
                String::new()
            } else {
                separator
            };
            None
        };
        items.insert(index, item.clone());
        Ok((index, item, shifted))
    }

    /// Put an item back at `index` as an edit recorded it: the item with
    /// its own text before it, the one after it with `shifted`.
    fn put(&mut self, index: usize, item: Item, shifted: Option<String>) {
        let (Node::Array { items, .. } | Node::Object { items, .. }) = self else {
            return;
        };
        let index = index.min(items.len());
        if let (Some(before), Some(next)) = (shifted, items.get_mut(index)) {
            next.before = before;
        }
        items.insert(index, item);
    }

    /// Take out the item at `index`; the item after it, if any, takes
    /// over the text the removed one had before it, and what it had is
    /// returned beside the item.
    fn take(&mut self, index: usize) -> (Item, Option<String>) {
        let (Node::Array { items, .. } | Node::Object { items, .. }) = self else {
            unreachable!("an edit only names containers");
        };
        let item = items.remove(index);
        let shifted = items
            .get_mut(index)
            .map(|next| std::mem::replace(&mut next.before, item.before.clone()));
        (item, shifted)
    }

    /// The text this container puts between items: what it already uses,
    /// or `, `.
    fn separator(&self) -> String {
        let (Node::Array { items, .. } | Node::Object { items, .. }) = self else {
            return ", ".to_owned();
        };
        items
            .iter()
            .skip(1)
            .map(|item| item.before.clone())
            .find(|before| before.contains(','))
            .unwrap_or_else(|| {
                // One item, or none: as wide as its own lead.
                match items.first() {
                    Some(first) if first.before.contains('\n') => format!(",{}", first.before),
                    _ => ", ".to_owned(),
                }
            })
    }

    fn write(&self, out: &mut String) {
        match self {
            Node::Scalar { raw, .. } => out.push_str(raw),
            Node::Array { items, tail } => {
                out.push('[');
                for item in items {
                    out.push_str(&item.before);
                    out.push_str(&item.key);
                    item.node.write(out);
                }
                out.push_str(tail);
                out.push(']');
            }
            Node::Object { items, tail } => {
                out.push('{');
                for item in items {
                    out.push_str(&item.before);
                    out.push_str(&item.key);
                    item.node.write(out);
                }
                out.push_str(tail);
                out.push('}');
            }
        }
    }

    pub fn value(&self) -> Value {
        match self {
            Node::Scalar { value, .. } => value.clone(),
            Node::Array { items, .. } => {
                Value::Array(items.iter().map(|i| i.node.value()).collect())
            }
            Node::Object { items, .. } => Value::Object(
                items
                    .iter()
                    .map(|i| (i.key_name().unwrap_or_default(), i.node.value()))
                    .collect(),
            ),
        }
    }

    /// A node for a value the editor made, laid out under `indent`: a
    /// container spans lines when it holds a container or more than a
    /// few items, as a hand would write it.
    pub fn from_value(value: &Value, indent: &str) -> Node {
        match value {
            Value::Array(values) => {
                let inner = format!("{indent}  ");
                let items: Vec<Node> = values.iter().map(|v| Node::from_value(v, &inner)).collect();
                let lines =
                    values.len() > 4 || values.iter().any(|v| v.is_array() || v.is_object());
                let mut out = Vec::new();
                for (i, node) in items.into_iter().enumerate() {
                    let before = match (i, lines) {
                        (0, false) => String::new(),
                        (0, true) => format!("\n{inner}"),
                        (_, false) => ", ".to_owned(),
                        (_, true) => format!(",\n{inner}"),
                    };
                    out.push(Item {
                        before,
                        key: String::new(),
                        node,
                    });
                }
                Node::Array {
                    items: out,
                    tail: if lines && !values.is_empty() {
                        format!("\n{indent}")
                    } else {
                        String::new()
                    },
                }
            }
            Value::Object(map) => {
                let inner = format!("{indent}  ");
                let lines = map.len() > 3 || map.values().any(|v| v.is_array() || v.is_object());
                let mut out = Vec::new();
                for (i, (key, v)) in map.iter().enumerate() {
                    let before = match (i, lines) {
                        (0, false) => String::new(),
                        (0, true) => format!("\n{inner}"),
                        (_, false) => ", ".to_owned(),
                        (_, true) => format!(",\n{inner}"),
                    };
                    out.push(Item {
                        before,
                        key: format!("{}: ", Value::String(key.clone())),
                        node: Node::from_value(v, &inner),
                    });
                }
                Node::Object {
                    items: out,
                    tail: if lines && !map.is_empty() {
                        format!("\n{indent}")
                    } else {
                        String::new()
                    },
                }
            }
            scalar => Node::Scalar {
                raw: scalar.to_string(),
                value: scalar.clone(),
            },
        }
    }
}

impl Item {
    /// The key as a name, unquoted; `None` in an array.
    pub fn key_name(&self) -> Option<String> {
        let key = self.key.trim_end();
        let key = key.strip_suffix(':')?.trim_end();
        serde_json::from_str::<String>(key).ok()
    }
}

/// A JSON reader that keeps what it reads.
struct Parser<'a> {
    text: &'a str,
    at: usize,
}

impl Parser<'_> {
    fn error(&self, message: &str) -> ParseError {
        ParseError {
            at: self.at,
            message: message.to_owned(),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.at).copied()
    }

    /// Whitespace from here, as text.
    fn whitespace(&mut self) -> String {
        let start = self.at;
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.peek() {
            self.at += 1;
        }
        self.text[start..self.at].to_owned()
    }

    fn node(&mut self) -> Result<Node, ParseError> {
        match self.peek() {
            Some(b'[') => self.array(),
            Some(b'{') => self.object(),
            Some(_) => self.scalar(),
            None => Err(self.error("a value")),
        }
    }

    fn array(&mut self) -> Result<Node, ParseError> {
        self.at += 1;
        let mut items = Vec::new();
        let mut before = self.whitespace();
        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(Node::Array {
                items,
                tail: before,
            });
        }
        loop {
            let node = self.node()?;
            items.push(Item {
                before,
                key: String::new(),
                node,
            });
            let gap = self.whitespace();
            match self.peek() {
                Some(b',') => {
                    self.at += 1;
                    before = format!("{gap},{}", self.whitespace());
                }
                Some(b']') => {
                    self.at += 1;
                    return Ok(Node::Array { items, tail: gap });
                }
                _ => return Err(self.error("`,` or `]`")),
            }
        }
    }

    fn object(&mut self) -> Result<Node, ParseError> {
        self.at += 1;
        let mut items = Vec::new();
        let mut before = self.whitespace();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(Node::Object {
                items,
                tail: before,
            });
        }
        loop {
            let key_start = self.at;
            if self.peek() != Some(b'"') {
                return Err(self.error("a key"));
            }
            self.string()?;
            self.whitespace();
            if self.peek() != Some(b':') {
                return Err(self.error("`:`"));
            }
            self.at += 1;
            self.whitespace();
            let key = self.text[key_start..self.at].to_owned();
            let node = self.node()?;
            items.push(Item { before, key, node });
            let gap = self.whitespace();
            match self.peek() {
                Some(b',') => {
                    self.at += 1;
                    before = format!("{gap},{}", self.whitespace());
                }
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Node::Object { items, tail: gap });
                }
                _ => return Err(self.error("`,` or `}`")),
            }
        }
    }

    /// A string from its opening quote past its closing one.
    fn string(&mut self) -> Result<(), ParseError> {
        self.at += 1;
        loop {
            match self.peek() {
                Some(b'"') => {
                    self.at += 1;
                    return Ok(());
                }
                Some(b'\\') => self.at += 2,
                Some(_) => self.at += 1,
                None => return Err(self.error("the end of a string")),
            }
        }
    }

    fn scalar(&mut self) -> Result<Node, ParseError> {
        let start = self.at;
        if self.peek() == Some(b'"') {
            self.string()?;
        } else {
            while let Some(c) = self.peek() {
                if matches!(c, b',' | b']' | b'}' | b' ' | b'\t' | b'\n' | b'\r') {
                    break;
                }
                self.at += 1;
            }
        }
        let raw = &self.text[start..self.at];
        let value: Value = serde_json::from_str(raw).map_err(|e| ParseError {
            at: start,
            message: e.to_string(),
        })?;
        Ok(Node::Scalar {
            raw: raw.to_owned(),
            value,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SHOW: &str = r#"{
  "format": 1, "name": "t", "size": [8, 8],
  "layers": [
    {"name": "a", "type": "shape", "x": 1.0, "y": 2},
    {"name": "b", "type": "shape", "x": 3, "y": 4.50}
  ],
  "scenes": []
}
"#;

    #[test]
    fn a_document_reads_back_byte_for_byte() {
        let doc = Document::parse(SHOW).unwrap();
        assert_eq!(doc.text(), SHOW);
        assert_eq!(doc.value(), serde_json::from_str::<Value>(SHOW).unwrap());
    }

    #[test]
    fn a_set_changes_one_number_and_nothing_else() {
        let mut doc = Document::parse(SHOW).unwrap();
        doc.set(&Pointer::parse("/layers/0/x").unwrap(), json!(7))
            .unwrap();
        assert_eq!(doc.text(), SHOW.replace(r#""x": 1.0"#, r#""x": 7"#));
        assert!(doc.is_dirty());
        assert!(doc.undo());
        assert_eq!(doc.text(), SHOW, "undo gives the text back, 1.0 included");
        assert!(!doc.is_dirty());
        assert!(doc.redo());
        assert_eq!(doc.text(), SHOW.replace(r#""x": 1.0"#, r#""x": 7"#));
    }

    #[test]
    fn items_come_and_go_with_their_separators() {
        let mut doc = Document::parse(SHOW).unwrap();
        let layers = Pointer::parse("/layers").unwrap();
        doc.insert(
            &layers.then(Step::Index(1)),
            json!({"name": "c", "type": "shape"}),
        )
        .unwrap();
        let text = doc.text();
        assert!(
            text.contains(
                "{\"name\": \"a\", \"type\": \"shape\", \"x\": 1.0, \"y\": 2},\n    {\"name\": \"c\", \"type\": \"shape\"},\n    {\"name\": \"b\""
            ),
            "{text}"
        );
        doc.remove(&layers.then(Step::Index(0))).unwrap();
        doc.remove(&Pointer::parse("/layers/1/y").unwrap()).unwrap();
        doc.insert(&Pointer::parse("/size/2").unwrap(), json!(9))
            .unwrap();
        doc.insert(&Pointer::parse("/scenes/0").unwrap(), json!({"name": "s"}))
            .unwrap();
        assert!(doc.text().contains("\"size\": [8, 8, 9]"), "{}", doc.text());
        assert!(
            doc.text().contains("\"scenes\": [{\"name\": \"s\"}]"),
            "{}",
            doc.text()
        );
        while doc.undo() {}
        assert_eq!(doc.text(), SHOW);
        while doc.redo() {}
        assert!(doc.text().contains("\"size\": [8, 8, 9]"));
    }

    #[test]
    fn a_new_container_is_laid_out_under_its_place() {
        let mut doc = Document::parse(SHOW).unwrap();
        doc.insert(
            &Pointer::parse("/layers/2").unwrap(),
            json!({"name": "g", "type": "group", "children": [{"name": "c", "type": "shape"}]}),
        )
        .unwrap();
        let text = doc.text();
        assert!(
            text.contains("},\n    {\n      \"name\": \"g\",\n      \"type\": \"group\",\n      \"children\": [\n        {\"name\": \"c\", \"type\": \"shape\"}\n      ]\n    }\n  ],"),
            "{text}"
        );
    }

    #[test]
    fn errors_say_what_is_wrong() {
        let mut doc = Document::parse(SHOW).unwrap();
        assert_eq!(
            doc.set(&Pointer::parse("/nowhere").unwrap(), json!(1)),
            Err(EditError::NotFound(Pointer::parse("/nowhere").unwrap()))
        );
        assert_eq!(
            doc.insert(&Pointer::parse("/layers/9").unwrap(), json!(1)),
            Err(EditError::OutOfRange(Pointer::parse("/layers/9").unwrap()))
        );
        assert_eq!(
            doc.insert(&Pointer::parse("/name").unwrap(), json!(1)),
            Err(EditError::Exists(Pointer::parse("/name").unwrap()))
        );
        assert!(Document::parse("{\"a\": }").is_err());
        assert_eq!(Pointer::parse("/a~1b/0").unwrap().to_string(), "/a~1b/0");
    }

    /// A small deterministic generator, so the test needs no crate.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: usize) -> usize {
            (self.next() % n.max(1) as u64) as usize
        }
    }

    /// Every pointer in the tree, with what it points at.
    fn pointers(node: &Node, at: Pointer, out: &mut Vec<(Pointer, bool)>) {
        match node {
            Node::Scalar { .. } => out.push((at, false)),
            Node::Array { items, .. } => {
                out.push((at.clone(), true));
                for (i, item) in items.iter().enumerate() {
                    pointers(&item.node, at.then(Step::Index(i)), out);
                }
            }
            Node::Object { items, .. } => {
                out.push((at.clone(), true));
                for item in items {
                    pointers(
                        &item.node,
                        at.then(Step::Key(item.key_name().unwrap())),
                        out,
                    );
                }
            }
        }
    }

    #[test]
    fn a_thousand_random_edits_undo_to_the_original() {
        let original = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/mini/show.json"
        ))
        .unwrap();
        let mut doc = Document::parse(&original).unwrap();
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        let mut made = 0;
        while made < 1000 {
            let mut all = Vec::new();
            pointers(&doc.root, Pointer::default(), &mut all);
            let (path, container) = all[rng.below(all.len())].clone();
            let value = match rng.below(4) {
                0 => json!(rng.below(100)),
                1 => json!(rng.below(100) as f64 / 4.0),
                2 => json!(format!("v{}", rng.below(1000))),
                _ => json!({"k": [1, {"deep": true}]}),
            };
            let done = match rng.below(3) {
                0 if !path.0.is_empty() => doc.set(&path, value).is_ok(),
                1 if container => {
                    let step = match doc.get(&path) {
                        Some(Node::Array { items, .. }) => Step::Index(rng.below(items.len() + 1)),
                        _ => Step::Key(format!("n{}", rng.below(50))),
                    };
                    doc.insert(&path.then(step), value).is_ok()
                }
                2 if !path.0.is_empty() => doc.remove(&path).is_ok(),
                _ => false,
            };
            if done {
                made += 1;
                // The text always re-reads to the same tree.
                let again = Document::parse(&doc.text()).unwrap();
                assert_eq!(again.value(), doc.value(), "after {made} edits");
            }
        }
        assert_eq!(doc.edits().len(), 1000);
        let edited = doc.text();
        while doc.undo() {}
        assert_eq!(doc.text(), original);
        assert!(!doc.is_dirty());
        while doc.redo() {}
        assert_eq!(doc.text(), edited);
        while doc.undo() {}
        assert_eq!(doc.text(), original);
    }
}
