# Repository Guidelines

## Product Vision & Scope
The suite is open-core for SMEs, optimized for high performance and low resource usage by staying 100% Rust. As laid out in `docs/research/plan.md` and `docs/research/codex-workflow.md`, CRM and HR launch first through a single `suite-server` binary (GraphQL + CLI).

## Project Structure & Module Organization
Follow the blueprint in `docs/research/codex-workflow.md`: `apps/suite-server` for Axum/async-graphql entrypoints, `platform/*` for reusable crates (authn, authz/Cedar, db, billing), `products/{crm,hr}` for domain logic, and `frontend/` for the Dioxus hub with lazy CRM/HR bundles. Align new modules (`db/entities`, `graphql/queries`, OIDC middleware) with the SeaORM and Postgres patterns captured in the research notes.

## Build, Test, and Development Commands
- `cargo check && cargo build --workspace`: compile every crate before committing.
- `DATABASE_URL=postgres://... cargo run -p suite-server -- serve --dry-run`: start the binary locally (defaults to `0.0.0.0:8080`).
- `sea-orm-cli migrate refresh && cargo test` (or `sqlx migrate run` if raw SQL migrations are chosen): keeps schema + code synchronized with RLS.
- `cargo fmt && cargo clippy --all-targets --all-features`: formatting + linting gate prior to PRs.

## Coding Style & Naming Conventions
Use four-space indentation and `rustfmt`. Stick to `snake_case` for functions/modules, `PascalCase` for types, and `SCREAMING_SNAKE_CASE` for env vars. Name crates by responsibility (`platform::authn`, `products::hr::timesheets`) and keep Axum routers under `routes/`, GraphQL objects in `graphql/`, and SeaORM entities in `db/entities`.

## Testing Guidelines
Work spec-first as suggested in `docs/research/codex-workflow.md`, keeping unit tests beside source and integration tests under `tests/`. Exercise GraphQL paths with `async-graphql` request tests plus SeaORM fixtures, rerun migrations in CI, enforce RLS behavior with Postgres/testcontainers suites, and snapshot `schema.graphql` whenever contracts change. Track performance budgets through the lightweight load targets described in the docs.

## Commit & Pull Request Guidelines
Continue the short, imperative commits already in `git log`, optionally scoping (`hr: add leave policy RLS`). Reference `codex/tasks/<id>.yml` or linked issues, summarize CRM vs HR impact, and attach `cargo test`/lint output. PRs should be atomic so each Codex work order remains reviewable and should include schema diffs or screenshots if UI/GraphQL behavior changes.

## Security & Configuration Tips
Store OIDC secrets, Cedar policy seeds, and billing keys outside git (loading via `.env`); document the variables inside `docs/research/codex-workflow.md`. Middleware must default to deny-by-default, GraphQL resolvers should call `authz.check()`, and any public routes (health, login callback) need justification inside the PR.
