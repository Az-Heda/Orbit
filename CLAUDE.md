# CLAUDE.md

Guidance for Claude Code when working on **Orbit**, a local-first Windows productivity app (calendar + kanban + timers + Claude copilot). See [README.md](README.md) for the overview and [ROADMAP.md](ROADMAP.md) for the plan and current phase.

## Stack

- Tauri 2, Rust backend, Svelte 5 + TypeScript frontend, Tailwind CSS
- SQLite via SQLx (WAL, FTS5), migrations in `src-tauri/migrations/`
- Claude Messages API via `reqwest` + `serde` (no official Rust SDK)
- Secrets in Windows Credential Manager

## Planned layout

```
src/                  Svelte frontend (components/, routes/, stores/, lib/)
src-tauri/src/
  commands/           Thin Tauri IPC handlers (calendar, tasks, projects, timers, ai)
  domain/             Entities and rules, no I/O
  services/           Business logic; used by BOTH the UI commands and AI tools
  infrastructure/     database/ (repositories), ai/ (Claude client), notifications/
  state.rs            App state
src-tauri/migrations/ SQL migrations
```

## Architecture rules

1. **Layering**: commands → services → repositories → SQLite. Commands stay thin. No SQL outside repositories. Services depend on repository traits, not concrete SQL.
2. **One code path for UI and AI.** AI tools call the same services as UI commands. Never add AI-only mutation logic.
3. **Claude never gets DB access.** Only explicitly registered tools, each with a JSON schema and server-side validation of IDs, dates, ranges and enum values.
4. **Confirmation model**: read tools run freely. Mutating tools return a *proposed change* (preview/diff) that the user confirms before it is applied. Delete and bulk operations always require confirmation.
5. **Every mutation writes to `activity_log`** (actor: user or ai, before/after), which powers undo and AI auditability.
6. **Timers store timestamps** (start, pause offsets) and derive elapsed time from the system clock. Never rely on JS `setInterval` as the source of truth. Handle sleep/resume.
7. **Secrets**: API key lives only in the Rust backend / Credential Manager. Never log it, never send it to the frontend.
8. **Privacy**: AI context sharing is explicit and configurable. Send the minimum needed (e.g. current board, selected task), not the whole DB.
9. **Data model**: UUID primary keys, foreign keys on, soft delete where appropriate, store timestamps in UTC and render in local time.

## Feature-specific rules

- **Inbox / quick capture**: raw captures go into `inbox_items` unchanged (store the original text). Triage (manual or AI) converts an item into a task or event and links back to it; never discard the original. AI triage follows the normal preview/confirm rule, and may be set to auto-apply only for low-risk creates if the user opts in.
- **Recurrence**: store the recurrence rule (RFC 5545 RRULE string plus timezone and `dtstart`) on the series, not expanded rows. Expand occurrences on read for a requested range (consider the `rrule` crate). Per-occurrence edits/skips are stored as exceptions. Handle DST by expanding in the series' local timezone. Reminders are derived from occurrences, and must survive app restarts and sleep/resume.
- **Calendar interop**: `.ics` import/export first (consider the `icalendar` crate); imports are deduplicated by UID. External calendars are **read-only and opt-in**: implemented as ICS feed subscriptions (`ExternalCalendarService`, table `external_calendars`, events tagged `external_calendar_id`). The feed URL is a secret: it lives only in the credential store (`infrastructure/feeds.rs`), never in the DB, never returned to the frontend, never in error text. Feed events are refused by every edit/delete path via `ensure_editable` (so the AI cannot touch them either); a feed event never overwrites a user event with the same UID. A sync writes one summary `activity_log` row, not one per event. OAuth (Google/Outlook sign-in) is not implemented; if added, tokens go to the credential store like the API key. Orbit stays fully usable offline with no account.
- **Notes**: markdown stored in `notes`, linked via `note_links` (polymorphic link to task, event, project). Indexed with FTS5 (keep the index in sync via triggers). AI note tools (`search_notes`, `get_note`, `create_note`) return snippets, not full dumps, and respect the AI context-sharing settings: notes are not sent to the API unless the user allows it. Editing or deleting a note through AI requires confirmation.

- **Task depth**: subtasks via `parent_id` on `tasks`; dependencies in `task_dependencies` (blocked_by). Reject cycles in the service layer (validate on every add). Completing a parent or moving a blocked task to an active/Done column should warn, not silently succeed. Templates are stored as structured task trees and instantiated through the normal task service (so activity_log and validation apply). AI planning tools must exclude or flag blocked tasks.
- **Stats**: computed from `focus_sessions`, `tasks` and `activity_log` with SQL aggregates in a stats service; no separate denormalized store at first. Never send stats data to the AI unless context sharing allows it.
- **Widget customization**: layout and theme are stored in `settings` and applied to the widget window via events. The widget is a separate Tauri window; keep its frontend bundle small. Click-through mode must always have an escape hatch (tray menu or hotkey).
- **AI memory**: `ai_memories` rows are plain user-readable text, fully viewable, editable and deletable in settings. The AI may propose a memory via `remember`, but it is saved only after user confirmation. Inject memories into the system prompt with a size cap, and never store secrets, API keys or sensitive personal data.
- **Automation rules**: a local engine with a closed set of triggers (task moved/created/completed, timer ended, time-based/cron, inbox item added) and actions (create task/note, move task, start timer, notify). Actions go through the same services as the UI and are logged to `activity_log` with actor `automation`. Guard against loops (rule-triggered changes must not retrigger the same rule; add a depth/rate limit) and allow disabling each rule. The AI may draft rules, but the user must confirm them. Planned (Phase 6): action chains and named notification channels (Windows/Telegram/Discord, each with a template); channel tokens/webhook URLs go in the credential store, never the DB/logs/frontend/AI, and delivery failures must not block the engine.
- **Global search**: a search service over FTS5 indexes for tasks, events, notes, projects and inbox, returning typed results with a score. UI and the AI `search_all` tool share it. Keep it responsive (debounce, limit results) and keyboard-navigable.

## Backups, import and updates (Phase 5)

- **Backup format** is a SQLite file made with `VACUUM INTO '<literal>'` (`infrastructure/database/backup.rs`). Bound parameters silently do not work there, so the path is an escaped literal and the file's existence is checked afterwards. In-memory DBs write nothing. Backup file names carry a UTC stamp and kind (`auto|manual|pre-import|pre-update`); only `auto` is pruned.
- **Import/restore** never touches the open DB: the verified file (integrity check, Orbit schema, migration version not newer than ours) is staged as `import-pending.db` and swapped in by `apply_pending_import` at startup *before* the pool opens, after a `pre-import` backup.
- **Updater** (`services/update_service.rs`, `domain/update.rs`): custom, no Tauri updater plugin, no installers. Install only if the minisign signature of `SHA256SUMS` verifies against the key in `src-tauri/update-public-key.txt` and the zip's SHA-256 matches; download URLs must be under the project's release area over https; no key means "offer the release page". Swap is rename-based with rollback; take a `pre-update` backup first. The check sends no user data and is opt-out (`update.check_enabled`).

## Desktop shell and background work

- **Events**: services publish `DomainEvent`s on an in-process bus *after* the mutation commits (task created/moved/completed, timer ended, inbox added). Only the automation engine and the UI relay subscribe; nothing may depend on events for correctness.
- **Scheduler** (`services/scheduler.rs`): ticks every ~20 s, settles timers, fires reminders and time-based rules. It compares against the clock and persists idempotency keys in `fired_keys`, so restart/sleep never double-fires; reminders missed within 6 h fire once as "Missed", older ones drop. Inject `Clock` and `Notifier` so it stays testable; never use JS timers for this.
- **Shell** (`src/shell.rs`, `commands/desktop.rs`): tray, widget/palette/capture windows, global shortcuts, autostart. Keep it thin glue; logic belongs in tested services. Every new window label must be added to `capabilities/default.json`. Windows (`widget`, `palette`, `capture`) are routes under `src/routes/(float)/`.
- Theme and widget config reach other windows via events (`theme-changed`, `widget-config-changed`); windows load them once at startup and then follow events (they never poll the DB for changes). Click-through must keep its escape hatches (tray item and shortcut).
- Shortcut registration failures are surfaced in Settings, never fatal. Avoid `Alt+Space`.

## AI integration notes

- Use tool use with a tool registry in `infrastructure/ai/`. Run the standard loop: send messages + tools, execute `tool_use` blocks, return `tool_result`, repeat until the model stops.
- Make the model name configurable in settings; do not hardcode it in call sites.
- Inject context (current date/time, timezone, active view, selected entity IDs) in the new user turn (not the system prompt, see below) so "this" and "tomorrow" resolve correctly.
- Stream responses to the frontend.
- Before writing Claude API code, load the `claude-api` skill for current model IDs and parameters.

### AI implementation notes (Phase 3)

- Transport: `infrastructure/ai/client.rs` (`ModelClient` trait, SSE parser/accumulator, retries before the stream starts). Assistant content blocks (incl. thinking, with signatures) are stored raw in `ai_messages` and replayed unchanged. Thinking/effort are only sent for models that support them (`supports_adaptive`).
- Tools live in `infrastructure/ai/tools.rs`: read tools run; write tools only `propose` (validate + preview) and are stored in `ai_proposals`. `apply` re-validates and runs the normal services with `Actor::Ai` inside `activity::BATCH.scope(proposal_id, ..)`, so every `activity_log` row of one proposal shares `batch_id` (the scope overrides repo-local batch ids). Undo (`AiRepository::undo_batch`) is all-or-nothing and refuses if an entity changed since.
- **Keep request history append-only.** A thinking block's signature covers the system prompt, the tool set and every earlier message (and the model that wrote it), so the system prompt and `tools` are constants. Per-turn context (date/time, timezone, view, selected ids, unshared categories, memories when changed) is a leading text block of the new user message, never in `system`. Never rewrite stored messages; unsigned (truncated) thinking blocks are not stored. Switching `ai.model` mid-conversation just means older reasoning is not read by the new model.
- Sharing categories (`SharingSettings`) are enforced when a tool runs (tools are all listed so the tool set stays constant; refused calls return an error). Timezone and ids from the UI are validated before they reach the prompt.
- Proposal previews include service warnings (open subtasks, blockers) as a `Warning` row; the user's confirmation of that preview is what forces the apply past them.
- Secrets go through `SecretStore` (Windows Credential Manager via `keyring-core`; elsewhere only `ANTHROPIC_API_KEY`). Commands can set/clear/report status, never return the key.
- Apply/undo of proposals is the only AI path that mutates; do not add AI-only mutation logic.
- Phase 4 tools: pure planning logic (free slots, ranking) lives in `domain/planning.rs` and is unit-tested; tools only gather data and format. `plan_day`/`daily_summary` check sharing per category inside the tool (they have no single category); `get_focus_sessions`/`link_focus_session` need the `focus` category. Bulk proposals (`create_subtasks`, `move_tasks`) apply inside one batch scope; a failed apply undoes its partial batch. `move_tasks` previews are always `destructive`. `TaskService::relocate_task` is the only cross-board move and refuses tasks with dependency links. Focus-session undo only reverts while the link is still what the AI set.

## Conventions

- Rust: `cargo fmt`, `cargo clippy -- -D warnings`, `thiserror` for domain errors, `anyhow` only at the edges. Unit-test services against an in-memory SQLite.
- Frontend: strict TypeScript, Svelte 5 runes, no business logic in components. Tauri commands are wrapped in typed functions under `src/lib/`.
- Migrations are append-only. Never edit a shipped migration.
- Keep commits small and scoped to one roadmap item where possible.

## Environment notes

- The user develops in WSL2 but targets **Windows**. Tauri Windows builds, WebView2, tray, global shortcuts and Credential Manager must be verified on Windows itself, not just under WSL/Linux. Prefer keeping core logic platform-independent so it is testable under Linux (`cargo test`), and isolate Windows-specific code behind small modules.

## Workflow

- Check [ROADMAP.md](ROADMAP.md) for the current phase before starting work; tick items off when done.
- When a new feature idea is accepted, add it to README.md, CLAUDE.md (if it affects architecture/rules) and ROADMAP.md together.

## Build and test

Package manager is **pnpm** (never npm/yarn; commit `pnpm-lock.yaml`).

```
pnpm install
pnpm tauri dev           # run app
cargo test               # in src-tauri/
cargo clippy -- -D warnings
pnpm run check           # svelte-check
```

## Building and releasing

- **"Build the project" means producing an executable, not an installer.** Always use `--no-bundle` (the Makefile does). Never run a bare `tauri build` and never add installer targets (NSIS/MSI/deb/AppImage/dmg) to builds or releases.
- `make windows` builds `bin/orbit.exe` (cross-compiled with `cargo-xwin` on Linux/WSL, native on Windows). `make linux` builds `bin/orbit` (Linux hosts only). `make clean` runs `cargo clean` and removes `bin/`. Windows is the primary target.
- **Builds hide build-machine paths** with `--remap-path-prefix` via `RUSTFLAGS` (Makefile and release workflow), since Rust embeds source paths in panic messages. Do not use the `trim-paths` profile key (nightly-only in current cargo). Migrations are pinned to LF in `.gitattributes`: sqlx checksums their bytes, and a CRLF checkout on the Windows runner broke existing databases.
- **No macOS builds or releases.**
- Releases: `.github/workflows/release.yml` runs when a tag like `v1.0.0` is pushed. The tagged commit must be on `main`, and the tag must equal the version in `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml`. It builds Windows and Linux executables and publishes them (zip / tar.gz) to a GitHub release. Planned auto-update (Phase 5): custom updater, never the Tauri updater plugin or installers; the app only installs a download whose checksum file has a valid ed25519/minisign signature (public key embedded, private key only in CI secrets), swaps the exe by rename with rollback, and the update check is opt-out and sends no user data.
