# Repository Guidelines

## Project Structure & Module Organization
Planning artifacts live under `codex/`: `tasks/*.yml` outline the incremental backlog, and `prompts/header.md` captures reusable instruction blocks for future agents. All research, architecture notes, and requirements (including the recommended Axum + SeaORM layout) are in `docs/research/`. When the Rust workspace is created, mirror the structure proposed in `docs/research/plan.md`—`src/db`, `src/graphql`, `src/auth`, and `src/routes`—so the CRM, HR, and accounting domains stay cleanly separated.

## Build, Test, and Development Commands
- `cargo check` then `cargo build --workspace`: verify the entire Axum/async-graphql stack compiles; use `--workspace` once multiple crates exist.
- `DATABASE_URL=postgres://... cargo run --bin server`: boot the API locally on `0.0.0.0:8080` as envisioned in the plan.
- `cargo fmt && cargo clippy --all-targets --all-features`: formatting plus lint coverage before every commit.
- `cargo test --workspace -- --nocapture`: execute resolver, SeaORM, and integration suites; keep output visible for async debugging.

## Coding Style & Naming Conventions
Default to four-space indentation and `rustfmt` output. Prefer `snake_case` for functions/modules, `PascalCase` for types, and `SCREAMING_SNAKE_CASE` for env constants. Organize Axum routers in `routes/mod.rs`, GraphQL queries/mutations/types under `graphql/`, and SeaORM entities in `db/entities`. Introduce submodules by CRM concept (contacts, deals, payroll) and keep files under 300 lines to match the modular approach described in the research documents.

## Testing Guidelines
Unit tests live alongside modules (`mod tests`) while integration tests belong in `tests/`. Use `async-graphql` request tests plus SeaORM fixture builders to cover GraphQL flows and data access. When editing migrations, run `sea-orm-cli migrate fresh && cargo test` to ensure RLS and tenant constraints behave. Snapshot GraphQL schemas whenever mutation contracts change and update `docs/research/codex-workflow.md` with new scenarios.

## Commit & Pull Request Guidelines
The Git log currently consists of short imperative statements (“Initial commit”, “add initial tasks”); continue that tone and add scopes when helpful (`db: add contact entities`). Reference the relevant `codex/tasks` identifier or GitHub issue in the body. Pull requests should summarize architectural impact, note touched directories, include `cargo test` results, and attach screenshots or schema diffs if the GraphQL surface shifts.

## Security & Configuration Tips
Never commit `.env`. Load `DATABASE_URL`, `JWT_AUDIENCE`, issuer URLs, and signing keys via `dotenvy` or launch scripts, and document any temporary defaults inside `docs/research/codex-workflow.md`. Middleware should deny-by-default; when creating health or auth callback routes, explain the reasoning in the PR to keep reviewers aware of exposure.
