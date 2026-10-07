//! The show's named lists the inspector edits row by row: the keys that
//! fire triggers, and the variables it declares with their starting
//! values. A row is a name and a value; renaming one keeps its value.

use serde_json::Value;

use crate::document::{Document, EditError, Part, Pointer};

/// One of the show's lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum List {
    /// A key's name, as a browser gives it, to the trigger it fires.
    Keys,
    /// A variable's name to the value it starts at.
    Variables,
}

impl List {
    /// The keys down to the list from the show's root.
    fn path(self) -> &'static [&'static str] {
        match self {
            List::Keys => &["input", "keys"],
            List::Variables => &["variables"],
        }
    }

    /// Its heading in the inspector.
    pub fn heading(self) -> &'static str {
        match self {
            List::Keys => "KEYS",
            List::Variables => "VARIABLES",
        }
    }

    /// What its names and values are, for an empty row's fields.
    pub fn hints(self) -> (&'static str, &'static str) {
        match self {
            List::Keys => ("key", "trigger"),
            List::Variables => ("name", "starts at"),
        }
    }

    fn pointer(self) -> Pointer {
        self.path().iter().fold(Pointer::default(), |at, key| {
            at.then(Part::Key((*key).to_owned()))
        })
    }

    /// The rows as the document writes them, in its order: each name with
    /// its value as a row shows it.
    pub fn rows(self, document: &Document) -> Vec<(String, String)> {
        let Some(Value::Object(entries)) = document.get(&self.pointer()).map(|n| n.value()) else {
            return Vec::new();
        };
        entries
            .into_iter()
            .map(|(name, value)| (name, show(&value)))
            .collect()
    }

    /// What was typed as a row's value, as the document gets it: a
    /// trigger's name, or a variable's number, `true`, `false` or text.
    pub fn parse(self, typed: &str) -> Result<Value, String> {
        let typed = typed.trim();
        match self {
            List::Keys if typed.is_empty() => Err("a trigger is needed".to_owned()),
            List::Keys => Ok(Value::from(typed)),
            List::Variables => Ok(match typed {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                _ => match typed.parse::<f64>() {
                    Ok(n) if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 => {
                        Value::from(n as i64)
                    }
                    Ok(n) if n.is_finite() => Value::from(n),
                    _ => Value::from(typed),
                },
            }),
        }
    }

    /// Write the row `name` with `value`, in place of the row `was` when
    /// it is renamed (or `None` for a new row), as one undo step; the
    /// list, and the `input` it sits in, are made when missing.
    pub fn put(
        self,
        document: &mut Document,
        was: Option<&str>,
        name: &str,
        value: Value,
    ) -> Result<(), String> {
        // A key's name is as a browser gives it: `" "` is the space bar.
        let name = match self {
            List::Keys => name,
            List::Variables => name.trim(),
        };
        if name.is_empty() {
            return Err("a name is needed".to_owned());
        }
        let list = self.pointer();
        let at = list.then(Part::Key(name.to_owned()));
        let renamed = was.filter(|was| *was != name);
        if renamed.is_some() && document.get(&at).is_some() {
            return Err(format!("{name:?} is there already"));
        }
        document.begin_step();
        let done = (|| -> Result<(), EditError> {
            if let Some(was) = renamed {
                document.remove(&list.then(Part::Key(was.to_owned())))?;
            }
            if document.get(&at).is_some() {
                return document.set(&at, value);
            }
            // The first of the list's keys the show does not write gets
            // the rest built round the row.
            let path = self.path();
            let mut parent = Pointer::default();
            for (depth, key) in path.iter().enumerate() {
                let next = parent.then(Part::Key((*key).to_owned()));
                if document.get(&next).is_none() {
                    let mut row = serde_json::Map::new();
                    row.insert(name.to_owned(), value);
                    let built = path.get(depth + 1..).unwrap_or_default().iter().rev().fold(
                        Value::Object(row),
                        |inner, key| {
                            let mut object = serde_json::Map::new();
                            object.insert((*key).to_owned(), inner);
                            Value::Object(object)
                        },
                    );
                    return document.insert(&next, built);
                }
                parent = next;
            }
            document.insert(&at, value)
        })();
        document.end_step();
        done.map_err(|error| error.to_string())
    }

    /// Take the row `name` out; a list left empty goes too.
    pub fn remove(self, document: &mut Document, name: &str) -> Result<(), EditError> {
        let mut keys = self.path().to_vec();
        keys.push(name);
        crate::fields::remove_path(document, &Pointer::default(), &keys)
    }
}

/// A row's value as its field shows it: a text as itself, anything else
/// as JSON.
fn show(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_row_is_added_renamed_and_removed() {
        let mut document = Document::parse(r#"{"name": "t", "size": [8, 8]}"#).unwrap();
        List::Keys
            .put(&mut document, None, "ArrowRight", json!("next"))
            .unwrap();
        assert_eq!(
            document.value()["input"],
            json!({"keys": {"ArrowRight": "next"}})
        );
        List::Keys
            .put(&mut document, Some("ArrowRight"), "", json!("next"))
            .unwrap_err();
        List::Keys
            .put(&mut document, Some("ArrowRight"), "Enter", json!("go"))
            .unwrap();
        assert_eq!(
            List::Keys.rows(&document),
            [("Enter".to_owned(), "go".to_owned())]
        );
        assert!(document.undo(), "a rename is one step");
        assert_eq!(
            List::Keys.rows(&document),
            [("ArrowRight".to_owned(), "next".to_owned())]
        );
        List::Keys.remove(&mut document, "ArrowRight").unwrap();
        assert!(
            document.value().get("input").is_none(),
            "an empty input goes"
        );
    }

    #[test]
    fn a_variable_starts_at_what_was_typed() {
        let mut document = Document::parse(r#"{"name": "t", "size": [8, 8]}"#).unwrap();
        for (name, typed) in [
            ("speed", "12"),
            ("ratio", "0.5"),
            ("on", "true"),
            ("label", "hi"),
        ] {
            let value = List::Variables.parse(typed).unwrap();
            List::Variables
                .put(&mut document, None, name, value)
                .unwrap();
        }
        assert_eq!(
            document.value()["variables"],
            json!({"speed": 12, "ratio": 0.5, "on": true, "label": "hi"})
        );
        let mut other = Document::parse(r#"{"variables": {"a": 1}}"#).unwrap();
        assert!(
            List::Variables.put(&mut other, None, "a", json!(2)).is_ok(),
            "a new row named as one there sets it"
        );
        assert_eq!(other.value()["variables"]["a"], 2);
        List::Variables
            .put(&mut other, None, "b", json!(1))
            .unwrap();
        assert!(
            List::Variables
                .put(&mut other, Some("b"), "a", json!(1))
                .is_err(),
            "a rename onto a row there is refused"
        );
    }
}
