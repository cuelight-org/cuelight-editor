//! Every trigger, variable and value of every example show renamed: the
//! show and its driver say as much about themselves as before, under
//! the new name.

// Test code throughout, so clippy lets it panic as tests do.
#![cfg(test)]

use std::path::{Path, PathBuf};

use cuelight_editor_core::log;
use cuelight_editor_core::opened::Opened;
use cuelight_editor_core::renames::{self, DRIVER, Kind};
use serde_json::Value;

/// The examples checkout's show folders, beside this repository or
/// where `CUELIGHT_EXAMPLES` says; none without a checkout.
fn example_shows() -> Vec<PathBuf> {
    let dir = std::env::var_os("CUELIGHT_EXAMPLES")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../cuelight-examples")
        });
    let Ok(catalog) = std::fs::read_to_string(dir.join("examples.json")) else {
        eprintln!("no cuelight-examples checkout: no show is renamed");
        return Vec::new();
    };
    let catalog: Value = serde_json::from_str(&catalog).unwrap();
    catalog["categories"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|c| c["examples"].as_array().unwrap())
        .map(|e| dir.join(e["path"].as_str().unwrap()))
        .collect()
}

/// What the audit says of a show and its driver, how many of each.
fn said(json: &str, files: &[String], driver: Option<&str>) -> usize {
    let driver = driver.map(|d| cuelight_loader::Driver::from_json(d).unwrap());
    log::audit(json, files, driver.as_ref()).len()
}

#[test]
fn every_name_of_every_example_renamed_says_as_much_as_before() {
    for path in example_shows() {
        let mut opened = Opened::from_path(&path).unwrap();
        let files: Vec<String> = opened.files.keys().cloned().collect();
        let driver = renames::driver_text(&opened.document, &opened.files);
        let driver_value: Option<Value> =
            driver.as_deref().map(|d| serde_json::from_str(d).unwrap());
        let text = opened.document.text();
        let before = said(&text, &files, driver.as_deref());
        let show = opened.engine.show().unwrap().clone();
        let mut names: Vec<(Kind, String)> = Vec::new();
        names.extend(show.triggers().into_iter().map(|t| (Kind::Trigger, t)));
        names.extend(show.variables.keys().map(|v| (Kind::Variable, v.clone())));
        names.extend(show.values.keys().map(|v| (Kind::Value, v.clone())));
        for (kind, name) in names {
            let to = format!("{name}_renamed");
            let value = opened.document.value();
            let plan = renames::plan(kind, &name, &to, &value, driver_value.as_ref())
                .unwrap_or_else(|e| panic!("{}: {name}: {e}", path.display()));
            let mut document = opened.document.clone();
            renames::apply(&plan, &mut document, driver.as_deref()).unwrap();
            let renamed = document.text();
            let renamed_driver = document.file(DRIVER).map(str::to_owned).or(driver.clone());
            let at = format!("{}: {} {name}", path.display(), kind.word());
            assert_eq!(
                said(&renamed, &files, renamed_driver.as_deref()),
                before,
                "{at}"
            );
            opened.engine.load_show_tolerant(&renamed).unwrap();
            let after = opened.engine.show().unwrap();
            match kind {
                Kind::Trigger => {
                    assert!(after.triggers().contains(&to), "{at}");
                    assert!(!after.triggers().contains(&name), "{at}");
                }
                Kind::Variable => assert!(after.variables.contains_key(&to), "{at}"),
                Kind::Value => assert!(after.values.contains_key(&to), "{at}"),
            }
            assert_eq!(
                serde_json::from_str::<Value>(&renamed)
                    .unwrap()
                    .to_string()
                    .len(),
                value.to_string().len()
                    + (to.len() - name.len())
                        * plan.uses.iter().filter(|u| u.file != DRIVER).count(),
                "{at}: only the names changed"
            );
        }
        opened.engine.load_show_tolerant(&text).unwrap();
    }
}
