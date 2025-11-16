## Progress Summary
- Migrated the backend away from raw `sqlx` to SeaORM: `platform/db` now exposes a `DatabaseConnection` pool plus tenant-aware helpers, the Axum state and `/health` endpoint call SeaORM APIs, and CLI bootstrap/setup uses those abstractions.
- Introduced a SeaORM-powered `migration` crate; `cargo run -p server -- migrate up|down`, startup pending-migration checks, and `m20240101_000001_init` all route through the shared migrator while legacy SQL executes via `execute_unprepared`.
- Reworked the automated tests around the new ORM: the `/health` unit test exercises a live connection, and `tests/rls_isolation.rs` spins up a Postgres testcontainer, runs migrations, seeds org/user fixtures, and validates tenant RLS rules.
- Unified the HTTP stack on Axum 0.8 by bumping workspace dependencies (`axum`, `axum-extra`, `tower`, `tower-http`) so `async-graphql-axum` no longer drags in a conflicting version, and ensured `cargo build` completes cleanly.
- Replaced the legacy `CookieJar` + `CookieManagerLayer` approach with `axum_extra::extract::cookie::PrivateCookieJar` tied to `AppState` via `FromRef`, letting login/callback/logout/GraphQL handlers read and write encrypted cookies directly.
- Hardened session handling (expiration comparisons via `with_timezone`) and refreshed the `/health` probe/CLI imports to the new APIs, leaving only the pre-existing `GraphqlData::pool` warning after `cargo build`.

## Rust File Tree
```
./apps/suite-server/src/main.rs
./entity/src/lib.rs
./entity/src/memberships.rs
./entity/src/orgs.rs
./entity/src/sessions.rs
./entity/src/users.rs
./migration/src/lib.rs
./migration/src/m20240101_000001_init.rs
./migration/src/m20240102_000002_sessions_and_default_org.rs
./platform/api/src/lib.rs
./platform/authn/src/lib.rs
./platform/authz/src/lib.rs
./platform/db/src/lib.rs
./platform/obs/src/lib.rs
./products/crm/src/lib.rs
./products/hr/src/lib.rs
./server/src/config.rs
./server/src/graphql/me.rs
./server/src/graphql/mod.rs
./server/src/http.rs
./server/src/main.rs
./tests/lib.rs
./tests/rls_isolation.rs
```
