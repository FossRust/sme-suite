Below is a **practical, high‑leverage playbook** to build your CRM + HR (single Rust binary, Postgres, Dioxus, GraphQL) **with Codex** as your coding agent—covering setup, guardrails, task templates, and day‑1→day‑14 execution. I assume you’ll use OIDC, Cedar, RLS, async‑graphql, and Dioxus with lazy‑loaded app modules.

---

## 1) Set up Codex correctly (so it actually ships code)

**What Codex gives you now**

* A **coding agent** that reads your repo, edits files, runs commands/tests in its own **cloud sandbox** and proposes PRs. You can also drive it from a **CLI/IDE extension** (VS Code/Cursor/Windsurf). ([OpenAI][1])
* **Quickstart**: connect your GitHub repo in Codex “Cloud agent” environment settings; then use the CLI (`codex`) to launch an interactive terminal UI that inspects/edits your code. ([OpenAI Developers][2])
* Pick the **GPT‑5‑Codex** model if available for best coding performance (enabled for Codex users; Responses API). ([OpenAI][3])

**Do this once**

1. Create a **clean monorepo** (see §2). Push to GitHub.
2. Go to Codex **Cloud agent** page → connect the repo → set **env vars/secrets** (DATABASE_URL, STRIPE keys, OIDC client secrets). ([OpenAI Developers][2])
3. Install **Codex CLI** locally; run `codex` in the repo to authenticate and open the TUI. ([OpenAI Developers][4])
4. In Codex settings, **limit edit scope** to `apps/*`, `platform/*`, `migrations/*`, `frontend/*` and **disallow** `infra/` & license files to prevent accidental edits.
5. Turn on **“require tests to pass before PR merge”** in your GitHub repo; Codex will still propose PRs but they won’t auto‑merge without green CI.

> Tip: If Codex service blips, check OpenAI status; there was a Codex outage in May 2025 that recovered. ([OpenAI Status][5])

---

## 2) Minimal repo layout Codex can navigate

```
fossrust-suite/
  Cargo.toml                # workspace
  apps/
    suite-server/           # single binary: CLI + HTTP GraphQL + jobs
  platform/                 # reusable crates (authn, authz, db, billing, etc.)
    authn/
    authz/
    db/
    home/
    …
  products/
    crm/
    hr/
  migrations/               # SQL (sqlx migrate)
  frontend/                 # Dioxus app (web/desktop/mobile), lazy modules per app
    src/
      app.rs
      hub/                  # Universal Home
      crm/                  # lazy chunk
      hr/                   # lazy chunk
  .github/workflows/ci.yml  # tests, lints, fmt, migrations, e2e
  Makefile / justfile
```

---

## 3) Guardrails so Codex can work safely and fast

* **Spec‑first**: put crisp ACCEPTANCE.md per task; Codex plans → implements → runs tests.
* **Golden tests**: unit + integration + GraphQL contract tests; no PR merges unless green.
* **Trunk‑based**: `main` protected; Codex branches: `codex/<task-id>`.
* **Static checks**: `cargo clippy -D warnings`, `cargo fmt --check`, `sqlx migrate check`.
* **Performance budgets**: add a quick **k6** or **bombardier** smoke test target to gate PRs.
* **Schema lock**: generate `schema.graphql` from `async-graphql`; diff in CI; PR fails if breaking change unless labeled `api-break`.

---

## 4) Codex “work orders” (templates you’ll actually use)

Below are **copy‑paste task prompts** you’ll feed Codex (CLI/IDE). Keep each atomic; attach the files it should read first.

### 4.1 Bootstrap single binary + CLI

**Title:** Scaffold `suite-server` (Axum + async-graphql + Clap)
**Acceptance:**

* `suite-server` binary with subcommands: `serve`, `migrate`, `seed`.
* `/health` (HTTP) returns 200; `/graphql` serves schema with QueryRoot.
* OpenTelemetry + tracing; graceful shutdown.
* CI runs `cargo test` + `cargo run -- serve --dry-run`.

### 4.2 Postgres + RLS + migrations

**Acceptance:**

* `platform/db` with sqlx pool and `SET LOCAL app.tenant_id`.
* Migrations for `users`, `orgs`, `memberships`, `policies`, `entitlements`, `audit`.
* RLS policies on all tenant tables.
* `suite-server migrate up/down` commands.
* Integration test spins ephemeral Postgres (testcontainers) and validates RLS.

### 4.3 AuthN (OIDC, configurable)

**Acceptance:**

* `platform/authn`: generic OIDC (discovery, JWKS cache, session cookie BFF).
* Config supports multiple providers (`auth0`, `cognito`, `keycloak`, …).
* `GET /login?provider=...` → OIDC code+PKCE → sets HttpOnly cookie.
* `Query.me` returns user + org + entitlements.

### 4.4 AuthZ (Cedar in‑proc)

**Acceptance:**

* `platform/authz` using `cedar-policy`.
* Policy store in DB (global/org scope) + hot reload via LISTEN/NOTIFY.
* GraphQL **guards** that call `authz.check()` and map to GraphQL errors.

### 4.5 GraphQL Hub + namespaced apps

**Acceptance:**

* `Query { hub, crm, hr, me, apps }`; `Mutation { crm, hr }`; `Subscription { hubEvents }`.
* `hub.overview` aggregates KPI cards + notifications + actions + recents.
* APQ (Automatic Persisted Queries) + GET caching; SSE subscriptions endpoint.

### 4.6 CRM minimal vertical slice

**Acceptance:**

* Entities: `account`, `contact`, `deal`, `activity`.
* Mutations: `createDeal`, `updateDealStage`.
* DataLoader for `User`, `Account`; p95 < 80ms on `hub.overview` with 20 deals.

### 4.7 HR minimal vertical slice

**Acceptance:**

* Entities: `employee`, `leave_request`, `timesheet`.
* Mutations: `requestLeave`, `approveLeave`.
* RLS + policy ensures managers see only their team.

### 4.8 Dioxus frontend shell + lazy modules

**Acceptance:**

* SPA with routes `/:org/home`, `/:org/crm/*`, `/:org/hr/*`.
* **Lazy‑load** CRM/HR bundles; show skeletons while loading.
* GraphQL client with APQ; SSE for hub events.
* Desktop build (Tauri) + web build; one codebase.

> Each work order: Codex **creates plan → diffs → runs tests** in its cloud sandbox and proposes a PR. You review/merge. ([OpenAI][1])

---

## 5) Concrete snippets Codex should produce (targets)

### 5.1 `suite-server` CLI skeleton

```rust
#[derive(clap::Parser)]
enum Cmd {
  Serve { #[arg(long, default_value_t=8080)] port: u16 },
  Migrate { #[arg(long, default_value="up")] direction: String },
  Seed,
}

fn main() -> anyhow::Result<()> {
  tracing_subscriber::fmt().with_target(false).init();
  match Cmd::parse() {
    Cmd::Serve { port } => runtime::serve(port),
    Cmd::Migrate { direction } => db::migrate(&direction),
    Cmd::Seed => scripts::seed(),
  }
}
```

### 5.2 async‑graphql schema merge

```rust
#[derive(MergedObject)] pub struct QueryRoot(HubQuery, CrmQuery, HrQuery, MeQuery);
#[derive(MergedObject)] pub struct MutationRoot(CrmMutation, HrMutation);
#[derive(MergedSubscription)] pub struct SubscriptionRoot(HubSub);

pub type AppSchema = Schema<QueryRoot, MutationRoot, SubscriptionRoot>;
```

### 5.3 RLS pattern (per request)

```sql
CREATE OR REPLACE FUNCTION current_tenant() RETURNS uuid
LANGUAGE sql STABLE AS $$ SELECT current_setting('app.tenant_id', true)::uuid $$;

ALTER TABLE deals ENABLE ROW LEVEL SECURITY;
CREATE POLICY by_org ON deals
USING (org_id = current_tenant()) WITH CHECK (org_id = current_tenant());
```

### 5.4 Cedar policy sample

```cedar
permit(
  principal in Role::"org_admin" || principal in Role::"sales",
  action in [Action::"deal.read", Action::"deal.write"],
  resource in Resource::"deal"
) when { principal.org_id == resource.org_id };
```

### 5.5 Dioxus lazy routes (sketch)

```rust
pub fn App(cx: Scope) -> Element {
  cx.render(rsx! {
    Router {
      Route { to: "/:org/home", element: Home {} }
      Route { to: "/:org/crm/*", element: Lazy::new(|| import_crm()) }
      Route { to: "/:org/hr/*",  element: Lazy::new(|| import_hr()) }
    }
  })
}
```

---

## 6) CI that Codex can satisfy

* **Jobs:** `fmt`, `clippy -D warnings`, `sqlx migrate check`, `cargo test --workspace`, **GraphQL schema diff**, **k6 smoke** (small), **Dioxus web build**, **Tauri desktop build (smoke)**.
* **Artifacts:** `schema.graphql`, APQ map, WASM bundle, desktop build.
* **PR gates:** green checks required; codeowners review for `platform/`.

---

## 7) Daily loop with Codex (what you actually do)

1. **Open a task issue** with acceptance criteria + links to relevant files.
2. In IDE or `codex` TUI, paste the **work order**; ask Codex to **plan → implement → run tests**.
3. Review the **diff & logs**; if tests fail, ask Codex to fix; otherwise **open PR**.
4. After merge, **benchmark** target routes; if regressions, open a “perf tune” work order.

Codex can run **multiple tasks in parallel** (cloud sandboxes), but keep changeset size small for easy review. ([OpenAI][1])

---

## 8) Initial 7‑day critical path (Codex‑driven)

* **Day 1:** Repo scaffold (§2), CLI (§4.1), DB+RLS (§4.2).
* **Day 2:** OIDC BFF (§4.3), `me` query, session cookies.
* **Day 3:** AuthZ Cedar (§4.4) + guards; seed policies.
* **Day 4:** GraphQL Hub (§4.5) with SSE + APQ.
* **Day 5:** CRM slice (§4.6) + tests + KPIs into `hub.overview`.
* **Day 6:** HR slice (§4.7) + tests + KPIs.
* **Day 7:** Dioxus shell + lazy modules (§4.8); deploy web to Amplify/Netlify; produce desktop artifact.

---

## 9) Prompts you’ll reuse (compact)

**“Plan mode”**

> Read: `/apps/suite-server`, `/platform/authn`, `/platform/authz`, `/products/crm`, `/products/hr`, `/frontend`. Goal: implement <task>. Output: (1) File-by-file plan with diffs; (2) Commands to run tests/build/migrations; (3) Rollback plan. Then wait.

**“Implement & test”**

> Apply the plan. Run: `cargo fmt && cargo clippy -D warnings && sqlx migrate run && cargo test -p suite-server -p crm -p hr`. If anything fails, fix and rerun until green. Then open PR titled: `<task> [codex]` with summary & risk.

**“Perf guard”**

> Run `k6 run tests/smoke.js`. Ensure `/graphql` `hub.overview` p95 < 80ms (local). If slower, profile hot resolvers and optimize DataLoader usage or indexes, then re-run.

---

## 10) Cost, speed, and model choice

* Use **GPT‑5‑Codex** inside Codex for coding heavy tasks; fall back to your usual chat model for brainstorming. ([OpenAI][3])
* Keep tasks **small** (≤300 LOC changes) so Codex’s planning/execution loop is rapid, and review remains trivial.
* Always pin versions (Rust toolchain, crate versions) so Codex works in a **deterministic** environment.

---

## 11) What not to let Codex do

* Don’t let it restructure the workspace or touch license/legal.
* Don’t accept autogenerated **secrets or keys**; you provide them via env.
* Don’t allow schema‑breaking changes without `api-break` label.

---

## 12) “Hello World” task to kick off (copy‑paste)

**Issue:** Implement GraphQL Hub skeleton with APQ + SSE
**Acceptance:**

* `Query { hub { overview { kpis { app key label value } } } }` returns fixed demo data.
* APQ enabled (GET with sha256 works; POST fallback works).
* `Subscription.hubEvents` pushes a demo notification every 10s.
* Dioxus `/acme/home` shows KPIs + live notifications.

**Codex work order:**

> Implement the task. Read `/apps/suite-server`, `/platform/api`, `/frontend/src`. Add APQ (GET param `extensions.persistedQuery.sha256Hash`), SSE endpoint `/graph/sse`, Dioxus components `frontend/src/hub/`. Create integration tests for APQ (GET/POST) and SSE (connect, receive message). Run all tests; open PR when green.

---

### References (what I relied on)

* Codex overview & capabilities (cloud agent, parallel tasks, repo editing/running/tests; CLI/IDE). ([OpenAI][1])
* Codex quickstart & cloud environment setup docs; CLI usage. ([OpenAI Developers][2])
* Latest code‑oriented model availability (GPT‑5‑Codex). ([OpenAI][3])
* OpenAI service status (Codex outage resolved). ([OpenAI Status][5])

If you want, I can draft the **first three Codex work orders** (scaffold server, DB+RLS, OIDC BFF) tailored to your repo name and exact crate choices so you can paste them straight into Codex and start shipping today.

[1]: https://openai.com/index/introducing-codex/?utm_source=chatgpt.com "Introducing Codex"
[2]: https://developers.openai.com/codex/quickstart/?utm_source=chatgpt.com "Quickstart"
[3]: https://openai.com/index/introducing-upgrades-to-codex/?utm_source=chatgpt.com "Introducing upgrades to Codex"
[4]: https://developers.openai.com/codex/cli/?utm_source=chatgpt.com "Codex CLI"
[5]: https://status.openai.com/incidents/01JW9TVDFN531DKHFG2GED7XTW?utm_source=chatgpt.com "Codex unavailable"

