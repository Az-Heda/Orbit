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
- **Calendar interop**: `.ics` import/export first (consider the `icalendar` crate); imports are deduplicated by UID. External Google/Outlook sync is **read-only and opt-in**; external events live in `external_calendars`-tagged rows, are never editable by the AI, and OAuth tokens are stored in Windows Credential Manager like the API key. Orbit stays fully usable offline with no account.
- **Notes**: markdown stored in `notes`, linked via `note_links` (polymorphic link to task, event, project). Indexed with FTS5 (keep the index in sync via triggers). AI note tools (`search_notes`, `get_note`, `create_note`) return snippets, not full dumps, and respect the AI context-sharing settings: notes are not sent to the API unless the user allows it. Editing or deleting a note through AI requires confirmation.

- **Task depth**: subtasks via `parent_id` on `tasks`; dependencies in `task_dependencies` (blocked_by). Reject cycles in the service layer (validate on every add). Completing a parent or moving a blocked task to an active/Done column should warn, not silently succeed. Templates are stored as structured task trees and instantiated through the normal task service (so activity_log and validation apply). AI planning tools must exclude or flag blocked tasks.
- **Stats**: computed from `focus_sessions`, `tasks` and `activity_log` with SQL aggregates in a stats service; no separate denormalized store at first. Never send stats data to the AI unless context sharing allows it.
- **Widget customization**: layout and theme are stored in `settings` and applied to the widget window via events. The widget is a separate Tauri window; keep its frontend bundle small. Click-through mode must always have an escape hatch (tray menu or hotkey).
- **AI memory**: `ai_memories` rows are plain user-readable text, fully viewable, editable and deletable in settings. The AI may propose a memory via `remember`, but it is saved only after user confirmation. Inject memories into the system prompt with a size cap, and never store secrets, API keys or sensitive personal data.
- **Automation rules**: a local engine with a closed set of triggers (task moved/created/completed, timer ended, time-based/cron, inbox item added) and actions (create task/note, move task, start timer, notify). Actions go through the same services as the UI and are logged to `activity_log` with actor `automation`. Guard against loops (rule-triggered changes must not retrigger the same rule; add a depth/rate limit) and allow disabling each rule. The AI may draft rules, but the user must confirm them.
- **Global search**: a search service over FTS5 indexes for tasks, events, notes, projects and inbox, returning typed results with a score. UI and the AI `search_all` tool share it. Keep it responsive (debounce, limit results) and keyboard-navigable.

## AI integration notes

- Use tool use with a tool registry in `infrastructure/ai/`. Run the standard loop: send messages + tools, execute `tool_use` blocks, return `tool_result`, repeat until the model stops.
- Make the model name configurable in settings; do not hardcode it in call sites.
- Inject context (current date/time, timezone, active view, selected entity IDs) in the system prompt so "this" and "tomorrow" resolve correctly.
- Stream responses to the frontend.
- Before writing Claude API code, load the `claude-api` skill for current model IDs and parameters.

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
- `make windows` builds `src-tauri/target/x86_64-pc-windows-msvc/release/orbit.exe` (cross-compiled with `cargo-xwin` on Linux/WSL, native on Windows). `make linux` builds `src-tauri/target/release/orbit` (Linux hosts only). `make clean` runs `cargo clean`. Windows is the primary target.
- **No macOS builds or releases.**
- Releases: `.github/workflows/release.yml` runs when a tag like `v1.0.0` is pushed. The tagged commit must be on `main`, and the tag must equal the version in `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml`. It builds Windows and Linux executables and publishes them (zip / tar.gz) to a GitHub release.
