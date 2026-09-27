# cuelight-editor

An editor for [cuelight](https://github.com/francisdb/cuelight) shows:
one Rust program, built with [iced](https://github.com/iced-rs/iced),
that runs on the desktop and in the browser.

It is at its first milestone: it opens a show and says what it found.
The plan it follows is the editor spec (goals, what it looks like, how
timelines and transitions are shown, and the work items in order).

## Running

```sh
cargo run --release                                # an empty window: open a show from the bar
cargo run --release -- ../cuelight-examples/deck   # or open one straight away
```

Open a show folder, a packed show (`.cuelight`) or a loose `show.json`
from the bar, or drop one on the window.

### In the browser

The same program compiled to `wasm32`, built with
[trunk](https://trunkrs.dev):

```sh
rustup target add wasm32-unknown-unknown
trunk serve --open          # builds, serves on http://localhost:8080 and reloads on change
trunk build                 # writes the page to dist/
```

The browser opens a packed show or a `show.json` through its file
picker; it cannot pick folders. Drawing needs WebGPU, as the cuelight web
player does.

## How it is built

- `src/opened.rs` opens a show in any of its forms into a `cuelight::Engine`,
  through `cuelight-loader`, and summarizes it. A show written for a
  newer format than the editor knows is refused with the version it
  wants.
- `src/dialog.rs` is the open dialog, native and web (`rfd`).
- `src/app.rs` is the window: the open bar, the summary, the status line,
  and file drops on the desktop.

Tests: `cargo test` opens a small show from `tests/fixtures/mini` as a
folder, as a pack and as a loose document, and checks the window through
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

MIT, see [LICENSE](LICENSE).
