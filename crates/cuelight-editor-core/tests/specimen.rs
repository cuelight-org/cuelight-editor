//! A font's sample line and specimen come out of the engine's own text
//! path: a bitmap font as its pixels, an outline font as its family.

// Test code throughout, so clippy lets it panic as tests do.
#![cfg(test)]

use std::path::{Path, PathBuf};

use cuelight_editor_core::opened::{self, Opened};
use cuelight_editor_core::specimen::{self, Drawn, Sizing};

fn fixture() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/typed"))
}

/// The examples checkout, beside this repository or where
/// `CUELIGHT_EXAMPLES` says.
fn examples() -> Option<PathBuf> {
    let dir = std::env::var_os("CUELIGHT_EXAMPLES")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../cuelight-examples")
        });
    dir.join("examples.json").exists().then_some(dir)
}

/// The rows of a raster as strings of `#` and `.`, from its alpha.
fn picture(drawn: &Drawn) -> Vec<String> {
    let Drawn::Raster(raster) = drawn else {
        panic!("not a raster: {drawn:?}");
    };
    (0..raster.height as usize)
        .map(|y| {
            (0..raster.width as usize)
                .map(|x| {
                    let a = raster.pixels[(y * raster.width as usize + x) * 4 + 3];
                    if a > 0 { '#' } else { '.' }
                })
                .collect()
        })
        .collect()
}

#[test]
fn a_bitmap_font_is_used_at_its_own_size_by_every_style_naming_it() {
    let opened = Opened::from_path(fixture()).unwrap();
    let show = opened::show(&opened).unwrap();
    assert_eq!(
        specimen::sizings(show, "tiny"),
        [Sizing {
            size: None,
            pixels: true,
            styles: vec!["loud".to_owned(), "plain".to_owned()],
        }]
    );
    assert!(specimen::sizings(show, "nope").is_empty());
}

#[test]
fn a_bitmap_fonts_sample_and_specimen_are_its_pixels() {
    let opened = Opened::from_path(fixture()).unwrap();
    let show = opened::show(&opened).unwrap();
    let sizing = specimen::sizings(show, "tiny").remove(0);
    // The sample line has A, B and spaces only in this font; whatever
    // else it says is skipped, so what is drawn is a few glyphs.
    let sample = specimen::sample(&opened.files, "tiny", &sizing).unwrap();
    let rows = picture(&sample);
    assert_eq!(rows.len(), 3, "{rows:?}");
    assert!(rows.iter().any(|r| r.contains('#')));
    // The specimen row that holds A and B shows them, 4 pixels apart.
    let lines = specimen::specimen(&opened.files, "tiny", &sizing);
    assert_eq!(lines.len(), specimen::rows().len());
    let ab = lines.iter().find(|l| l.text.starts_with('@')).unwrap();
    assert_eq!(
        picture(ab.drawn.as_ref().unwrap()),
        [".#..##.", "#.#.#.#", "###.##."],
        "{}",
        ab.text
    );
    // A row with none of the font's characters draws nothing.
    let none = lines.iter().find(|l| l.text.starts_with('0')).unwrap();
    assert_eq!(none.drawn, None);
}

#[test]
fn an_outline_font_is_pixels_when_asked_and_its_family_otherwise() {
    let Some(examples) = examples() else {
        eprintln!("no cuelight-examples checkout: the outline font is not tried");
        return;
    };
    let opened = Opened::from_path(&examples.join("features/text/pixel_font")).unwrap();
    let show = opened::show(&opened).unwrap();
    let sizings = specimen::sizings(show, "Tiny5-Regular");
    let crisp = sizings
        .iter()
        .find(|s| s.size == Some(8.0) && s.pixels)
        .expect("a style draws Tiny5 as pixels");
    let soft = sizings
        .iter()
        .find(|s| s.size == Some(8.0) && !s.pixels)
        .expect("a style draws Tiny5 as outlines");
    let sample = specimen::sample(&opened.files, "Tiny5-Regular", crisp).unwrap();
    let rows = picture(&sample);
    assert!(
        rows.len() >= 5 && rows.iter().any(|r| r.contains('#')),
        "{rows:?}"
    );
    assert_eq!(
        specimen::sample(&opened.files, "Tiny5-Regular", soft),
        Some(Drawn::Outline(Some(specimen::Family {
            name: "Tiny5".to_owned(),
            weight: 400,
            italic: false,
        })))
    );
}

#[test]
fn a_fonts_look_is_its_sample_and_a_specimen_per_size() {
    let opened = Opened::from_path(fixture()).unwrap();
    let show = opened::show(&opened).unwrap();
    let look = specimen::look(&opened.files, show, "tiny", false);
    let sizing = &look.sizings[0];
    assert_eq!(look.sizings.len(), 1);
    assert_eq!(look.sample, specimen::sample(&opened.files, "tiny", sizing));
    assert_eq!(
        look.specimens,
        [specimen::specimen(&opened.files, "tiny", sizing)]
    );
    // A font no style uses is still shown, at a size of its own.
    let unused = specimen::look(&opened.files, show, "nope", true);
    assert_eq!(unused.sizings, [Sizing::unused(true)]);
    assert_eq!(unused.sample, None, "no such font file");
}
