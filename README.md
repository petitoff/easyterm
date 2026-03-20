# easyterm

`easyterm` is a Rust workspace bootstrap for a Linux-first terminal emulator with a fast core and an SSH-oriented remote layer.

This repo currently implements the project skeleton, a headless terminal core, and a first GUI shell:

- `easyterm-core`: ANSI/VT parsing, grid model, cursor/state handling, scrollback, resize logic
- `easyterm-render`: renderer abstraction with GPU-first / CPU fallback selection
- `easyterm-remote`: SSH profile model and validation
- `easyterm-app`: config loading, session model, Linux PTY runtime, GUI windowing, and CLI fallbacks for inspection/debugging

## Workspace layout

```text
crates/
  easyterm-app/
  easyterm-core/
  easyterm-remote/
  easyterm-render/
```

## CLI commands

After installing Rust, the current bootstrap is intended to expose:

```bash
cargo run -p easyterm-app
cargo run -p easyterm-app -- gui
cargo run -p easyterm-app -- cli
cargo run -p easyterm-app -- sample-config
cargo run -p easyterm-app -- inspect-config ./easyterm.toml
cargo run -p easyterm-app -- replay ./fixtures/demo.ansi
cargo run -p easyterm-app -- capture-shell "printf 'hello from pty\n'"
cargo run -p easyterm-app -- capture-local /bin/sh -lc "printf 'hello\n'"
```

## Current scope

Implemented:

- terminal grid and cursor state
- ANSI color/style parsing
- line wrap and scrollback
- Linux PTY-backed local process execution
- GUI window with local shell tabs
- keyboard input, basic mouse tab switching, wheel scrolling, and basic text selection
- CLI passthrough fallback under `cli`
- renderer selection abstraction
- SSH profile parsing and validation
- TOML config model

Not implemented yet:

- splits UI
- actual GPU text renderer
- font shaping and font fallback
- SSH transport/session execution

## Toolchain

The workspace targets stable Rust through `rust-toolchain.toml`.
