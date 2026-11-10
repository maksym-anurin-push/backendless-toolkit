# Contributing

This file is for humans and coding agents (Cursor, Copilot, Claude, Codex, etc.).

## Development setup

See [README.md](./README.md#development).

## Database migrations

Schema changes live under [`database/`](./database/). Full conventions: [`database/README.md`](./database/README.md).

## Code

- Workspace crates: `grai_registry_bin`, `grai_registry_client`, `grai_registry_contract`, `grai_registry_runtime`.
- Prefer small, focused changes that match existing style.
- Build/test: `cargo build --locked`, `cargo test --frozen`.
