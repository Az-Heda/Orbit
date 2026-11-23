# Orbit Roadmap

Build order: local productivity first, desktop experience second, AI third, polish last. Get the local app and Claude tool calling working well before adding any extra infrastructure.

Legend: `[ ]` todo · `[x]` done

## Phase 0: Foundation

- [x] Scaffold Tauri 2 + Svelte 5 + TypeScript + Tailwind
- [x] SQLite setup with SQLx, WAL mode, foreign keys
- [x] Migration system and initial schema
- [x] Repository traits + service layer skeleton
- [x] Settings storage
- [x] Error handling and logging
- [x] `activity_log` plumbing

## Phase 1: Productivity core

**Kanban**
- [x] Projects, boards, columns, tasks (CRUD)
- [x] Priorities, due dates
- [x] Drag-and-drop
- [x] Subtasks and checklists
- [x] "Blocked by" dependencies with cycle detection
- [x] Task templates (e.g. "new project")

**Calendar**
- [x] Events CRUD
- [x] Day / week / month views
- [x] Deadlines shown from tasks
- [x] Recurring events and tasks (RRULE storage, expansion on read, per-occurrence exceptions, DST-safe)
- [x] `.ics` import and export (dedupe by UID)

**Notes**
- [x] Markdown notes and journal (CRUD)
- [x] Link notes to tasks, events and projects
- [x] FTS5 search with sync triggers

**Timers and dashboard**
- [x] Pomodoro and custom/countdown timers (timestamp-based, persistent)
- [x] Focus session history linked to tasks
- [x] Overview dashboard
- [x] Stats dashboard (focus hours per day/project, completions over time, streaks, heatmap)
- [x] Global search across tasks, events, notes, projects and inbox (FTS5-backed, keyboard-first)

## Phase 2: Desktop experience

- [x] System tray (keep running in background)
- [x] Floating always-on-top widget (clock, next event, active task, timer, quick input)
- [x] Global keyboard shortcut and command palette window (with global search)
- [x] Customizable widget: selectable blocks, light/dark/accent themes, opacity and click-through ambient mode
- [x] Automation rules engine (triggers, actions, loop protection, per-rule enable/disable)
- [x] Windows notifications (upcoming events, timer completion)
- [x] Sleep/resume handling for timers
- [x] Launch at startup option
- [x] Quick capture: global hotkey opens a mini input, items land in the Inbox
- [x] Inbox view with manual triage into task or event
- [x] Smart reminders for events and recurring items (persist across restart and sleep)

Logic (inbox, reminders, automation, timers, sleep catch-up) is unit-tested on Linux. The desktop shell (tray, transparent widget, click-through, global shortcuts, toasts, autostart) compiles for Windows but must be verified on a Windows machine.

## Phase 3: Claude copilot (MVP AI)

- [x] Claude client (`reqwest` + `serde`), streaming
- [x] API key storage in Windows Credential Manager
- [x] Tool registry with JSON schemas and validation
- [x] Read tools: `get_schedule`, `get_tasks`
- [x] Write tools: `create_event`, `update_event`, `create_task`, `move_task`, `delete_task`, `start_timer`
- [x] Confirmation previews / diffs; mandatory confirmation for delete and bulk actions
- [x] Contextual awareness (current board / selected task / current date and timezone)
- [x] Chat UI + persisted conversation history
- [x] Configurable AI context sharing (privacy settings)
- [x] Undo of AI actions via `activity_log`
- [x] Note tools: `search_notes`, `get_note`, `create_note` (respecting context-sharing settings)
- [x] Inbox tools: `get_inbox`, `triage_inbox_item` (AI files captured items as tasks/events, with preview)
- [x] Task tools: `add_dependency`, `create_subtask`, `create_task_from_template`; planning respects dependencies
- [x] Search tool: `search_all`
- [x] AI memory: editable memory list, `remember` / `forget` tools with confirmation, injected into context with a size cap
- [x] AI can draft automation rules (user confirms before enabling)

The Claude client (SSE parsing, tool loop, retries), tool validation, proposals/confirmation, sharing gates, undo and memory are unit-tested on Linux against a scripted model and a local fake server. **Not yet verified:** a real call with a live API key, and the Windows Credential Manager store (compiles for Windows via `cargo xwin check`). Server-side `fallbacks` are not enabled.

## Phase 4: Intelligence

All AI items below are tools in `infrastructure/ai/tools.rs` that go through the same services as the UI; writes are proposals the user confirms. Only the deterministic parts (free-slot search, ranking, relocation, feeds) are unit-tested; model behaviour was not exercised against the live API.

- [x] Natural-language scheduling ("block 2 hours tomorrow for studying"): `find_free_slots` + `create_event`
- [x] AI daily planning from events, tasks, priorities and deadlines: `plan_day`
- [x] Schedule optimization (find free slots for unfinished tasks): working-hours-aware `find_free_slots`, DST-safe
- [x] Task breakdown into subtasks: `create_subtasks` (one atomic, undoable proposal)
- [x] Smart organization (e.g. move all Rust-related tasks into the Rust project): `move_tasks` (cross-board, always flagged for confirmation; tasks with dependencies cannot leave their board)
- [x] Daily summary of completed/unfinished work: `daily_summary`
- [x] Context-aware "what should I work on next?" suggestions: `suggest_next_tasks` (overdue, due soon, priority, in progress; blocked tasks excluded)
- [x] Automatic association of focus sessions with tasks: a focus timer started without a task links to the single task in an active column (setting `timers.auto_link`); `get_focus_sessions` / `link_focus_session` fix up the rest (new "focus sessions" sharing category, off by default)
- [x] Read-only calendar subscriptions (opt-in): paste a Google/Outlook private iCal (ICS) address; events are tagged `external_calendar_id`, refreshed hourly, never editable (UI and AI), URL kept in Credential Manager
- [x] AI answers from notes ("what did I decide about auth?"): `search_notes` + `get_note` with citation guidance in the system prompt (respects the notes sharing setting)

## Phase 5: Polish and release

- [x] Automatic local backups: daily snapshot (`VACUUM INTO`) of `orbit.db` into `backups/`, keep the latest N (setting, default 7), manual / pre-import / pre-update backups are never pruned
- [x] Export / import data: export writes a SQLite snapshot to a chosen `.db` file; import (or restoring a backup) verifies the file (integrity check, Orbit schema, not newer than this version), takes a safety backup and swaps it in at the next start. Not JSON/CSV: the format is the database itself
- [x] FTS5 search across tasks, projects and notes (done in Phase 2: global search over FTS5 indexes kept in sync by triggers)
- [x] Auto-update (custom, no installer or Tauri updater plugin, since releases are a plain `orbit.exe`; Windows only at first). Implemented and unit-tested on Linux against a fake release source; the Windows swap and a real signed release are unverified
  - [x] Check the GitHub Releases API (`releases/latest`) on startup and daily, compare the tag to the running version with semver; unauthenticated, no user data, opt-out in Settings plus a "Check now" button
  - [x] Ask: dialog with version and release notes, buttons "Update", "Later" and "Skip this version" (skipped version stored in settings)
  - [x] Download the Windows zip asset (and the signed checksum list)
  - [x] Verify: the release workflow publishes `SHA256SUMS` and a minisign signature (private key in the `MINISIGN_SECRET_KEY` Actions secret); the app embeds the public key (`src-tauri/update-public-key.txt`) and refuses to install on any mismatch. **Needs a one-time key setup by the maintainer, see docs/RELEASING.md; until then the app only offers the release page**
  - [x] Swap without a helper script: rename `orbit.exe` to `orbit.exe.old`, move the new exe in, launch it and exit; the new version deletes `.old` on start; roll back on any failure; if the folder is not writable, fall back to opening the release page
  - [x] Back up `orbit.db` before installing an update (`pre-update` backup)
  - [ ] Later: Linux (`orbit` tar.gz) with the same flow; code signing against SmartScreen warnings
- [x] Documentation and onboarding: dismissible "Getting started" panel on the Overview page (`onboarding.done` setting), docs/USER_GUIDE.md, docs/RELEASING.md, README usage section

## Phase 6: Automation expansion

- [ ] Chained actions: a rule can have several "Then" steps that run in order (add/remove/reorder in the automation page; a failing step is logged and stops the chain)
- [ ] Notification channels: Settings page to create named channels of type Windows toast, Telegram (bot token + chat id) or Discord (webhook URL), each with a message template (placeholders such as `{{task.title}}`, `{{rule.name}}`) and a "Send test" button
- [ ] Automation action "Send a notification": pick the action in the first select, then a second select appears listing the saved channels by name; the channel's template is rendered with the trigger's data
- [ ] Channel secrets (bot token, webhook URL) live in the credential store, never in the DB, logs or frontend; delivery failures are logged to `activity_log`, rate-limited and never block the rule engine
- [ ] The AI may draft rules that use a channel by name, but never sees tokens or webhook URLs and cannot create or edit channels
- [ ] Follow-up: real OAuth sign-in for Google/Outlook calendars (needs registered client IDs; the Phase 4 ICS address covers read-only use meanwhile)

## MVP definition

Phases 0 to 3 plus Phase 5 (backups, export/import, updates, onboarding). Orbit ships as a single plain executable (`orbit.exe`); there is no installer, now or planned.

## Future / not planned yet

- Optional cloud sync (local-first; repository traits keep this possible)
- Mobile companion
- Orbit as an MCP server (Claude Desktop/Code operating Orbit via the same tool registry)
- Local LLM provider (Ollama) behind an AI provider abstraction
- Daily and weekly review ritual (morning plan, evening wrap-up, weekly retrospective)
- Voice input in the command palette (local speech-to-text)
- History browser with per-change and per-AI-batch revert
- Opt-in proactive nudges (dismissible suggestion cards, never auto-applied)

## Expansion ideas log

Accepted (already placed in the phases above): quick capture inbox, recurring tasks/events with smart reminders, calendar interop (`.ics` then read-only external sync), notes and journal, task depth (subtasks, dependencies, templates), stats dashboard, customizable widget, AI memory, automation rules, global search, automation expansion (chained actions, notification channels: Windows/Telegram/Discord).

Parked: MCP server, local LLM, review ritual, voice input, history browser, proactive nudges.

Declined: automatic time tracking from the active Windows window, habit tracker, energy/estimate-aware scheduling, focus mode.
