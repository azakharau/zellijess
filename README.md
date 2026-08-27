# zellijess

`zellijess` is an experimental terminal UI for browsing Zellij sessions, tabs,
and panes. It builds a searchable navigation tree and shows a snapshot or
near-live preview for the selected terminal pane.

The project is usable as a development preview. Discovery, parsing, filtering,
navigation, and selected-pane preview are implemented. Selecting an item with
`Enter` currently stages the corresponding action but does not switch sessions,
tabs, or panes.

## Requirements

- A current stable Rust toolchain
- Zellij available on `PATH` for runtime discovery and pane previews

## Run

```sh
cargo run -- demo
```

The demo uses fixture-backed navigation data while pane previews call the local
Zellij CLI. It exits cleanly without opening the UI when no interactive terminal
is available.

To inspect the Zellij commands available in the current environment:

```sh
cargo run -- status
```

`status` only runs read-only discovery commands.

## Controls

- `j` / `Down`: move down
- `k` / `Up`: move up
- `/`: filter the tree
- `Esc`: clear the filter, leave filter mode, or quit
- `Enter`: stage the selected navigation action
- `q`: quit from navigation mode
- `Ctrl-c`: quit from any mode

## Development

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

The runtime command contract and current UI boundaries are documented in
[`docs/runtime-contract.md`](docs/runtime-contract.md) and
[`docs/tui-shell.md`](docs/tui-shell.md).

## License

MIT
