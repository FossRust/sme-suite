# sme-suite

High-performance, low resource footprint SME suite (CRM, HR, CMS, Accounting, etc.) built in Rust. The open-core edition launches with CRM and HR slices sharing a single Axum + async-graphql backend.

For contributor workflow, architecture notes, and coding standards see [AGENTS.md](AGENTS.md).

## Quickstart

```bash
# Recommended: drop into the dev shell with latest toolchain + CLIs
nix develop

# Run the placeholder binary
cargo run -p suite-server -- serve --dry-run
```

Without Nix, install Rust 1.77+, `sea-orm-cli`, and `sqlx-cli` manually, then run the same cargo commands.

## Workspace Layout

- `apps/suite-server`: Axum/async-graphql binary exposing CLI subcommands (`serve`, `migrate`, `seed`).
- `platform/*`: reusable crates (`authn`, `authz`, `db`) that will power auth flows, Cedar checks, and SeaORM pools.
- `products/crm`, `products/hr`: domain-specific libraries for early CRM + HR experiences.
- `migrations/`: SQL or SeaORM migrations plus RLS policies.
- `frontend/`: Dioxus/Tauri shell with lazy CRM + HR bundles (not part of the Rust workspace yet).
- `.github/workflows`: CI wiring for fmt, clippy, and tests.

## Tooling

- `flake.nix` provides a reproducible dev shell with the latest stable Rust toolchain, SeaORM CLI, SQLx CLI, wasm-pack, just, and supporting libs.
- CI uses stable Rust with `cargo fmt`, `cargo clippy -- -D warnings`, and `cargo test --workspace`.
- Preferred test command locally: `cargo test --workspace --all-features`.
