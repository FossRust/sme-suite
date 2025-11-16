# FossRust Suite — Global Constraints (read before coding)

## Stack (Rust, single binary)
- Rust stable (≥ 1.91), edition 2024.
- Web: axum ^0.8, tower, tower-http.
- GraphQL: async-graphql ^7, async-graphql-axum ^7.
- DB: Postgres 18+, sqlx ^0.8 (runtime=tokio, tls=rustls), sqlx::migrate!.
- AuthN: openidconnect ^4, reqwest ^0.12 (rustls).
- AuthZ: (placeholder for Cedar), not part of first 3 tasks.
- Observability: tracing ^0.1, tracing-subscriber ^0.3, opentelemetry-otlp.
- CLI: clap ^4.5.
- Serialization: serde ^1, serde_json ^1.
- Error: anyhow ^1, thiserror ^1.

## Non-negotiables
- **Single binary** at `/server` providing CLI (`serve|migrate|seed|schema:print|apq:gen`) and HTTP.
- **Postgres as sole state**. No Redis, no external queues.
- **Multitenancy**: org-first, enforce **RLS**; per-request `SET LOCAL app.tenant_id`.
- **GraphQL-only** API; enable GET+APQ later; for now POST is fine.
- **Security**: HttpOnly cookie session (BFF), no tokens in the SPA; CORS locked down.
- **Perf budgets**: p95 < 80ms for simple queries; forbid unbounded lists (use Relay-style).
- **Code quality**: `cargo fmt`/`clippy -D warnings` must pass; no `unwrap()` in resolvers.

## Repo layout (target after these tasks)
fossrust-suite/
  Cargo.toml                      # workspace
  server/                         # single binary (CLI + HTTP + GraphQL + jobs)
  platform/                       # shared crates (db, api, obs, etc.)
  products/                       # (empty now; will add crm/hr later)
  sql/                            # migrations
  policies/                       # (placeholder)
  frontend/                       # (placeholder Dioxus app)
  .github/workflows/ci.yml        # CI (fmt, clippy, build, sqlx, tests)

## CI gates (each PR must pass)
- cargo fmt / clippy -D warnings / test
- sqlx migrate check (offline) + prepare
- GraphQL schema print + snapshot diff (no breaking changes unless labeled `api-break`)
- Simple k6/bombardier smoke ok (optional in these first 3)

## Style/structure
- Layered: `server/src/main.rs` wires routes; `server/src/graphql/` contains schema roots;
  `platform/db` exposes pooled connections and RLS helpers; `platform/api` holds GraphQL helpers.
- Use `tracing::instrument` for resolvers/handlers; add request-id middleware.

Do NOT:
- Introduce extra services.
- Hardcode secrets; use env/config only.
- Change license or workspace layout.
