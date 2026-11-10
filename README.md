[![Build Status](???.svg)](???)

# Backendless Toolkit

Backendless Toolkit provides CLI tools and MCP server to manage Backendless Applications via [Backendless REST API Documentation](https://backendless.com/docs/rest/)

## Development

1. Configure [credential provider](https://doc.rust-lang.org/cargo/reference/registry-authentication.html)
1. Create [access token](https://gitea.anurin.name/user/settings/applications) with **Package Read** permission
1. Authorize into registry
   ```shell
   TOKEN=abc123
   echo -n "Bearer ${TOKEN}" | cargo login --registry freemework
   ```
1. Work with code

```shell
cargo clean
cargo build --locked
cargo test --frozen
cargo run --frozen --package grai_registry_bin -- --backendless.1.url=https://eu-api.backendless.com --backendless.2.url=https://api-bl-k8s.baelab.net 
```

## Database CLI

Configure Console credentials in `.env` (see `.env-example`), then:

```shell
cargo build --locked --bin backendless-toolkit-db
./target/debug/backendless-toolkit-db show_tables
./target/debug/backendless-toolkit-db dump_table_structure MyTable
./target/debug/backendless-toolkit-db dump_table_data MyTable
```
