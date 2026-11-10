# Contributing

This file is for humans and coding agents (Cursor, Copilot, Claude, Codex, etc.).

## Development setup

See [README.md](./README.md#development).

## Code

- Workspace crates: `backendless_console_sdk`, `backendless_toolkit_bin`, `backendless_toolkit_contract`, `backendless_toolkit_runtime`.
- Prefer small, focused changes that match existing style.
- Build/test: `cargo build --locked`, `cargo test --frozen`.
- DB CLI (`backendless-toolkit-db`) reads Console credentials from `.env` (see `.env-example`).
