# Contributing

## Build, test, and lint

- Build: `cargo build`
- Run locally: `cargo run -- --config ~/.config/devops/config.toml`
- Install the binary from the repo root: `cargo install --path .`
- Test all: `cargo test`
- Run a single test: `cargo test <test_name_substring>`
- Format check: `cargo fmt --all -- --check`
- Clippy: `cargo clippy --all-targets -- -D warnings`
- Security checks, when the tools are available: `cargo audit` and `cargo deny --all-features check`.

The current baseline is clean — `cargo build`, `cargo test`, `cargo fmt --all -- --check`, and `cargo clippy --all-targets -- -D warnings` all pass without warnings or formatting drift. Keep it that way.

Before committing, always run:

```sh
cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test
```

A git pre-commit hook (`.githooks/pre-commit`) enforces these same checks. One-time setup:

```sh
git config core.hooksPath .githooks
```

## Coverage and security gates

CI currently gates pull requests and `main` on `cargo check --all-targets`,
`cargo clippy --all-targets -- -D warnings`, `cargo fmt --all -- --check`,
and `cargo test` across Linux x64/ARM, macOS, and Windows.

### Coverage gate plan

No coverage reporter or threshold gate is currently committed. Adding a tool or
external action such as `cargo llvm-cov`, `cargo tarpaulin`, `grcov`, or `Codecov`
requires maintainer approval because it installs or executes external tooling.
After approval, add a Linux coverage job to `.github/workflows/ci.build.yml`
that reuses `cargo test`, publishes a text and machine-readable report as a CI
artifact, starts as an informational gate to capture the baseline, and then
fails pull requests that reduce baseline coverage or miss an approved threshold.

Prioritize direct tests for these high-risk modules that currently lack local
test modules or have only indirect integration coverage:

- `src/state/actions/dispatch.rs` and `src/state/run.rs` — central action
  routing and event-loop orchestration.
- `src/client/auth.rs` and `src/client/http/{builds,definitions,approvals,retention,pull_requests}.rs`
  — Azure credential handling and ADO REST wrappers.
- `src/components/log_viewer/{follow,timeline,state}.rs` and
  `src/events/log_viewer.rs` — follow/inspect behavior, task selection, and
  collapse state.
- `src/events/common.rs` and `src/events/navigation.rs` — global navigation,
  confirmation, and shared input behavior.

### Security advisory review

`.github/workflows/ci.security.yml` runs `cargo audit` and
`cargo deny --all-features check` on pushes, pull requests, and the weekly
scheduled scan.
Advisory ignores must be mirrored in `.cargo/audit.toml` and `deny.toml` with
an advisory link, rationale, Reviewed date, Review-by date, and trigger for
early re-review. Treat the Review-by date as the expiry for the justification;
remove the ignore or refresh the review before release if the date has passed.

### Release checklist

Before publishing a release:

- Start from `main` with all release-impacting changes committed using
  Conventional Commits; if a pull request is used, keep it draft until it is
  ready for review and merge it before dispatching the release workflow.
- Run the release-candidate flow below before making the release non-draft.
- Run or confirm the required validation gates: `cargo fmt --all -- --check`,
  `cargo clippy --all-targets -- -D warnings`, and `cargo test`, plus the
  cross-platform build workflow on `main`.
- Confirm `cargo audit` and `cargo deny --all-features check` are passing, and
  refresh or remove any advisory ignore whose Review-by date has expired.
- Publish only archives listed in `SHA256SUMS`; verify the release workflow
  signs that manifest with the expected Sigstore identity and uploads both
  `SHA256SUMS` and `SHA256SUMS.cosign.bundle`.
- Verify the install scripts and `devops update` trust path still fails closed:
  the signed manifest is verified before archive hashes, and archive hashes are
  checked before extraction or replacement.

### Release-candidate flow

Use the release workflow's draft-release path as the release candidate for the
exact final `vX.Y.Z` tag. Do not create `vX.Y.Z-rc.N` tags with the current
workflow: version discovery is built around final semver tags, and prerelease
tags can confuse the next-version calculation unless the workflow is changed to
ignore them.

1. Prepare `main`.
   - Keep only intended release changes since the previous release.
   - Use concrete scoped Conventional Commits, for example
     `fix(update): retain rollback target until lock clears` or
     `docs(install): document signed checksum verification`.
   - Confirm the build, security, and install workflows are green for the source
     commit. If the install workflow skips self-update tests because there is no
     previous release, record that limitation in the draft release notes.
2. Create the RC draft.
   - Dispatch `.github/workflows/ci.release.yml` from `main` with the intended
     `bump` and `draft=true`.
   - Record the workflow run URL, source SHA, calculated tag, and archive list.
     Treat the draft release assets as `rc.1`.
   - If validation requires another candidate, delete the draft release and any
     generated tag, merge the fixes to `main`, and dispatch a fresh draft so the
     final promotion still uses one clean `vX.Y.Z` tag.
3. Validate the RC artifacts before publishing.
   - Confirm the release run completed every build matrix job and the
     `Verify release artifact trust path` step.
   - Download `SHA256SUMS`, `SHA256SUMS.cosign.bundle`, and all `devops-*`
     archives from the draft release with maintainer credentials. Verify the
     manifest signature with the identity in `SECURITY.md`, then run
     `sha256sum --check SHA256SUMS` from the asset directory.
   - Compare the asset set with the release workflow matrix: Linux x64/ARM
     tarballs, macOS x64/ARM tarballs, Windows x64/ARM zip files,
     `SHA256SUMS`, and `SHA256SUMS.cosign.bundle`. Remove any unexpected asset
     before promotion.
   - On at least one Unix host and one Windows host or CI runner, install from
     the verified RC archive into a disposable directory and run
     `devops version` and `devops --help`. Use the convenience scripts only when
     they can authenticate to the draft asset and local policy permits executing
     the checked-in script.
4. Check update and rollback behavior.
   - Before promotion, confirm `cargo test update` passes on `main` so the
     in-process signature and hash verification, staged install, update lock,
     pruning, and startup rollback tests match the RC source.
   - Confirm `.github/workflows/ci.install.yml` is green against the latest
     public release; its self-update jobs prove the currently published
     `devops update` path can advance from the previous release.
   - The full installer and `devops update` check for the new version can only
     run after publication because they intentionally target the latest
     non-draft release. Treat that run as an immediate post-promotion gate; if
     it fails, re-draft or delete the release, announce that the previous
     release remains supported, and cut a fixed RC from `main`.
5. Promote the RC.
   - Publish the existing draft release instead of running a second release
     workflow against the same tag.
   - If the version bump commit is not produced by automation because the RC was
     drafted first, update `Cargo.toml` and `Cargo.lock` on `main` with
     `chore(release): bump to X.Y.Z`.
   - Confirm GitHub marks the release non-draft, the tag points at the recorded
     source SHA, release notes mention any skipped gates, and
     `releases/latest` resolves to the promoted tag.

## High-level architecture

The codebase follows the [ratatui Component Architecture](https://ratatui.rs/concepts/application-patterns/component-architecture/) pattern.

### Data flow

1. **Input** — Terminal events are dispatched to per-view handlers in `events/`, which return an `Action` describing the intent.
2. **Action handling** — `state/actions/dispatch.rs` processes the action, spawning async API calls via helpers in `state/actions/spawn.rs`.
3. **Message handling** — Async results arrive as `AppMessage` variants (defined in `state/messages.rs`). `state/actions/messages.rs` applies them to `App` state.
4. **Rendering** — `render/mod.rs` delegates to each component's `draw_with_app()` method. Components never perform mutations or network calls.

### Module layout

| Module        | Purpose |
|---------------|---------|
| `client/`     | Azure DevOps REST layer — `auth.rs` (bearer tokens), `http/` (reqwest wrapper, per-domain submodules), `endpoints/` (URL builders), `models/` (ADO payload types). |
| `state/`      | Application state (`App` in `mod.rs`), per-view sub-states, `actions/` (dispatch → spawn → messages loop), `run.rs` (main event loop with `tokio::select!`). |
| `components/` | Self-contained UI components implementing the `Component` trait. Each view plus overlays (Header, Help, Settings) has its own module. |
| `events/`     | Keyboard/mouse event handling — split per-view with shared handlers in `events/common.rs`. Returns `Action` variants. |
| `render/`     | Shared rendering: `helpers.rs` (status icons, elapsed time, truncation), `theme.rs` (colors), `setup.rs` (first-run wizard), `columns.rs`/`table.rs` (column layout primitives). |
| `shared/`     | Cross-cutting infrastructure: `ListNav`, `RefreshState`, `Notifications`. |
| `config.rs`   | TOML configuration loading and validation. |
| `update.rs`   | Self-update mechanism (GitHub releases). |

### Adding a new view

1. Create `src/components/my_view.rs` implementing the `Component` trait.
2. Create `src/events/my_view.rs` with view-specific key handling.
3. Add the view state to `App` and a `View::MyView` variant in `state/mod.rs`.
4. Register rendering in `render/mod.rs` and event dispatch in `events/mod.rs`.

## Conventions

- **Network orchestration stays out of components.** Remote work follows `Action` → `spawn_*` helper in `state/actions/spawn.rs` → `AppMessage` → `handle_message` in `state/actions/messages.rs`. Components are render-only.
- **`log_generation` is a stale-response guard.** Timeline and log messages carry a generation counter; stale results from a previously selected build are silently dropped.
- **ADO status/result strings are case-insensitive.** Reuse `eq_ignore_ascii_case` and the shared render helpers.
- **Dashboard grouping uses raw ADO definition paths.** Root is stored as `\`; the user-facing `" / "` folder display is derived at render time.
- **Timeline flattening.** `LogViewer` flattens `Stage → Phase → Job → Task` into `Stage → Job row → Task`.
- **Collapse state is keyed by ADO record IDs**, not by visible row indices. Rebuild row lists after structural state changes.
- **Per-view sub-states.** `App` is decomposed into `data` (`CoreData`), `filters` (`FilterConfig`), `search` (`SearchState`), and view-specific component structs. Don't add fields to `App` that belong to a specific view.
- **List view column layout.** All list views source widths from shared schemas in `src/render/columns.rs` built on the `Column` / `ColumnWidth::{Fixed, Flex}` primitive in `src/render/table.rs`.
- **Test helpers.** `src/test_helpers.rs` provides factory functions used by both unit tests in `src/` and integration tests in `tests/`.

## Code comments

- Every `.rs` file starts with a `//!` module doc — a single sentence ending with a period.
- `///` doc comments start with a third-person present-tense verb ("Returns…", "Renders…") and end with a period.
- Inline `//` comments are sentence case, end with a period, and explain *why*, not *what*. Always include a space after `//`.
- Use the short divider form only: `// --- Section Name ---`. No full-width `// ----` or `// ====` dividers.
- No commented-out code; no empty doc comments.

## Reporting security issues

See [SECURITY.md](../SECURITY.md).
