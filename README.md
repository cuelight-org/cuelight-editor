# cuelight-editor

An editor for [cuelight](https://github.com/francisdb/cuelight) shows:
one Rust program, built with [iced](https://github.com/iced-rs/iced),
that runs on the desktop and in the browser.

It is at its first milestone: it opens a show, plays it on a stage
drawn by the engine's own renderer, and says what it found. The plan it
follows is the editor spec (goals, what it looks like, how timelines and
transitions are shown, and the work items in order).

## Running

```sh
cargo run --release                                # an empty window: open a show from the bar
cargo run --release -- ../cuelight-examples/deck   # or open one straight away
```

Open a show folder, a packed show (`.cuelight`) or a loose `show.json`
from the bar, or drop one on the window. The show plays with its
`test-driver.json`, sound included on the desktop; Space pauses and
plays, R restarts, `,` and `.` step a frame (a second with Shift), and
the playhead scrubs by replaying the show's inputs.

The panel on the left is how a show is driven: its triggers as
buttons, its variables as fields and toggles, its own values as
readouts, and what happened lately. Keys the show names, and presses on
its pressable layers, go to the show first (Ctrl reaches the editor's
own shortcuts); what you fire is recorded, so scrubbing back replays it.
The Driver switch in the bar turns the show's own driver off, so nothing
happens until you make it.

### In the browser

The same program compiled to `wasm32` is published from `main` at
https://francisdb.github.io/cuelight-editor/, and built locally with
[trunk](https://trunkrs.dev):

```sh
rustup target add wasm32-unknown-unknown
trunk serve --open          # builds, serves on http://localhost:8080 and reloads on change
trunk build                 # writes the page to dist/
```

The browser opens a packed show or a `show.json` through its file
picker, or straight from the page's URL: `?show=deck.cuelight` fetches
and opens that file. It cannot pick folders. Drawing needs WebGPU, as
the cuelight web player does.

## How it is built

Two crates in a workspace:

- `crates/cuelight-editor-core` is what the editor knows without a window,
  with no iced and no GPU, so it compiles in seconds and its tests run
  anywhere:
  - `opened.rs` opens a show in any of its forms into a `cuelight::Engine`,
    through `cuelight-loader`, and summarizes it. A show written for a
    newer format than the editor knows is refused with the version it
    wants.
  - `session.rs` plays a show: the engine, its driver and an anchored
    clock, as the players keep time.
- `crates/cuelight-editor` is the window:
  - `stage.rs` draws the show: an iced shader widget in which the engine's
    presenter and vello render the frame into a texture, blitted into the
    widget's rectangle. It is the players' own render path, so the stage
    looks like them.
  - `dialog.rs` is the open dialog, native and web (`rfd`), and the
    page's `?show=` opener.
  - `app.rs` is the window: the open bar with the transport, the stage
    beside the summary, the status line, and file drops on the desktop.

Tests: `cargo test --workspace` opens a small show from
`crates/cuelight-editor-core/tests/fixtures/mini` as a folder, as a pack
and as a loose document, and checks the window and the clock through
`iced_test`.

### Dependencies and versions

iced has no release on the wgpu version the engine draws with (wgpu 29,
through vello), so `Cargo.toml` pins iced to a commit of its master
branch, and pins the engine crates to a commit of cuelight main. When
wgpu moves, all three move together in one change.

To build against a local checkout of the engine:

```sh
cargo build --config 'patch."https://github.com/francisdb/cuelight".cuelight.path="../cuelight/crates/cuelight"' \
            --config 'patch."https://github.com/francisdb/cuelight".cuelight-loader.path="../cuelight/crates/cuelight-loader"'
```

CI builds and tests the desktop and builds the web page on every
change, with a size budget on the `.wasm`, and builds against the
engine's main branch weekly so a change there that breaks the editor
shows up within days.

## License

MIT, see [LICENSE](LICENSE). The bundled UI fonts, Atkinson Hyperlegible
and DM Mono, are under the SIL Open Font License 1.1
(`crates/cuelight-editor/fonts/`).
