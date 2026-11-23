# Orbit

A local-first personal productivity command center for Windows: **calendar + kanban + timers + an AI copilot that can operate all three.**

Orbit is not just another calendar or kanban app. The differentiator is a Claude-powered assistant that can read and modify your schedule and tasks through a controlled set of tools, with you approving every change that matters.

> Status: Phases 0-4 (foundation, productivity core, desktop experience, Claude copilot, intelligence) implemented; Windows-side verification of the desktop shell and a live-API check of the copilot are pending. See [ROADMAP.md](ROADMAP.md) for the plan and [CLAUDE.md](CLAUDE.md) for development guidance.

## Vision

Open your desktop and see your day at a glance in a small floating widget. Hit a global shortcut, type "Move my database task to tomorrow", and it's done, without opening the full app.

## Design decisions

| Decision | Choice |
| --- | --- |
| Form factor | **Hybrid**: full app plus lightweight floating widget and command palette |
| AI behavior | **Copilot**: acts when asked, always previews changes before applying |
| Data | **Local-first**: everything in SQLite on the device, no accounts, no server |
| Platform | Windows first (Tauri 2 is cross-platform, so others stay possible) |

## Modules

- **Calendar**: events, deadlines, time blocking, day/week/month views.
- **Kanban**: projects, boards, columns, tasks, priorities, drag-and-drop.
- **Timers**: Pomodoro, countdowns, custom timers, focus session history linked to tasks.
- **AI copilot**: chat and command palette that call typed tools on the Rust backend.
- **Notes**: markdown notes and journal, linked to tasks, events and projects, full-text searchable, readable by the AI for context.
- **Inbox / quick capture**: a global hotkey opens a tiny box; whatever you type lands in an Inbox and is filed as a task or event (by you or the AI).
- **Recurrence and reminders**: recurring events and tasks (every Monday, monthly, ...) with smart reminders.
- **Calendar interop**: `.ics` import/export, plus opt-in read-only subscriptions to Google/Outlook calendars through their private iCal address (no account sign-in; the address stays in the Windows Credential Manager).
- **Task depth**: subtasks, checklists, "blocked by" dependencies and reusable templates (e.g. a "new project" template). The AI respects dependencies when planning.
- **Stats dashboard**: focus hours per day/project, tasks completed over time, streaks and a heatmap calendar, all computed locally.
- **Customizable widget**: choose the blocks the floating widget shows (clock, next event, timer, active task, inbox count), light/dark/accent themes, and an opacity/click-through ambient mode.
- **AI memory**: a visible, editable list of preferences/facts the copilot remembers and includes in its context.
- **Automation rules**: local trigger/action rules (e.g. task moved to Done logs to the journal; timer ends starts a break; every Friday create a review task). Planned: chained "then" steps and named notification channels (Windows, Telegram, Discord) with templates.
- **Global search**: one fuzzy search across tasks, events, notes, projects and inbox, available in the palette and main window, keyboard-first.

## Desktop shortcuts

| Shortcut (default, configurable) | Action |
| --- | --- |
| `Ctrl+Alt+Space` | Command palette (search everything, jump anywhere) |
| `Ctrl+Alt+N` | Quick capture into the Inbox |
| `Ctrl+Alt+W` | Toggle widget click-through (escape hatch; also in the tray menu) |

Closing the main window keeps Orbit running in the tray. Enable "Launch at startup" in Settings to start hidden in the tray. `Alt+Space` is deliberately not used (Windows/PowerToys).

## Three windows

1. **Main window**: full workspace with all modules.
2. **Floating widget**: always-on-top compact panel with clock, next event, active task, timer, quick AI input.
3. **Command palette**: global shortcut (e.g. `Alt+Space`) for quick AI commands and actions.

## Tech stack

| Layer | Technology |
| --- | --- |
| Desktop framework | Tauri 2 |
| Backend | Rust |
| Frontend | Svelte 5 + TypeScript |
| Styling | Tailwind CSS |
| Database | SQLite (WAL mode, FTS5 for search) via SQLx |
| AI | Claude Messages API with tool use, called via `reqwest` + `serde` (no official Rust SDK) |
| Secrets | Windows Credential Manager |

## Architecture

```
Svelte frontend (Dashboard · Calendar · Kanban · Timers · AI chat)
        │  Tauri IPC (commands)
Rust core: commands → services → repositories
        │                         │
   AI service ── Claude API     SQLite (orbit.db)
        │
   Tool registry → same services the UI uses
```

Key principles:

- **Claude decides, Rust executes.** The model never touches the database. It only calls explicitly defined, validated tools that go through the same services as the UI.
- **Preview before apply.** Mutating AI actions are shown as a diff and confirmed; destructive or bulk actions always require confirmation.
- **Repository traits** keep storage swappable, so cloud sync could be added later without rewriting business logic.
- **Timers persist timestamps**, never JS intervals. Elapsed time is computed from the system clock, so timers survive window closure and sleep/resume.
- **API key stays in the backend**, stored in Windows Credential Manager, never in frontend code.
- **Explicit AI context.** Only what you choose to share leaves your machine, and only for AI requests.

## Data

`orbit.db` lives in the Windows app data directory. Tables: `projects`, `boards`, `columns`, `tasks`, `events`, `focus_sessions`, `settings`, `ai_conversations`, `ai_messages`, `activity_log`, plus `inbox_items`, `notes`, `note_links`, `external_calendars`, `task_dependencies`, `task_templates`, `ai_memories` and `automation_rules`. Subtasks use a parent reference on `tasks`. UUIDs everywhere, migrations, foreign keys, soft deletes, automatic backups.

## AI tools

Read tools run immediately. Every tool that changes data only creates a **proposal** (preview/diff) that you apply, reject or later undo from the chat; deletes are flagged. What the assistant can read is set per category in Settings (tasks, calendar, notes, inbox, memories; notes and inbox are off by default).

Initial set:

`get_schedule`, `get_tasks`, `create_event`, `update_event`, `create_task`, `move_task`, `delete_task`, `start_timer`.

Added by accepted ideas: `search_notes`, `get_note`, `create_note`, `get_inbox`, `triage_inbox_item`, `add_dependency`, `create_subtask`, `create_task_from_template`, `search_all`, `remember` / `forget` (AI memory, user-editable), `list_projects`, `list_templates`, `update_task`, `delete_event`, `draft_automation_rule`.

Phase 4 (planning): `find_free_slots`, `plan_day`, `suggest_next_tasks`, `daily_summary`, `get_focus_sessions`, `create_subtasks`, `move_tasks` (bulk, cross-project), `link_focus_session`. Notes questions use `search_notes` / `get_note`. Focus data is its own sharing category (off by default).

Setting the API key: paste it in Settings → Assistant (stored in the Windows Credential Manager). On non-Windows builds only the `ANTHROPIC_API_KEY` environment variable is used. The model name and effort are settings (default `claude-opus-5-5`, effort `medium`).

## Ideas parked for later

- Orbit as an **MCP server** so Claude Desktop/Code can operate it from outside.
- **Local LLM** provider (e.g. Ollama) as an offline/private alternative to the Claude API.
- **Daily and weekly review ritual** (morning plan, evening wrap-up, weekly retrospective).
- **Voice input** in the command palette via local speech-to-text.
- **History browser**: timeline of all changes with per-change and per-AI-batch revert.
- **Opt-in proactive nudges** (suggestion cards, never auto-applied).

Declined: automatic time tracking from the active window, habit tracker, energy/estimate-aware scheduling, focus mode (Do Not Disturb and blocking).

## Note on cost

Claude API usage is billed separately from a Claude subscription (Pro/Max does not include API credits).

## Getting started

Prerequisites: Rust toolchain, Node.js, [pnpm](https://pnpm.io), and the Tauri 2 prerequisites (on Windows: WebView2 and MSVC build tools; on Linux: `webkit2gtk-4.1`, GTK 3, libsoup 3, `libayatana-appindicator3` for the tray).

```
pnpm install
pnpm tauri dev
```

## Building

Orbit is built as a plain **executable, not an installer**.

| Command | Output |
| --- | --- |
| `make windows` | `bin/orbit.exe` |
| `make linux` | `bin/orbit` (Linux hosts only) |
| `make clean` | runs `cargo clean` and removes `bin/` |

On Linux/WSL, `make windows` cross-compiles using `clang`, `lld`, `llvm`, the `x86_64-pc-windows-msvc` Rust target and `cargo-xwin` (`cargo install --locked cargo-xwin`). On Windows it builds natively. Cross-compiled output should still be tested on Windows.

## Releases

Pushing a tag like `v1.0.0` (the commit must be on `main`, and the version must match `tauri.conf.json` and `Cargo.toml`) triggers the GitHub Actions release workflow. It builds Windows (`.zip` with `orbit.exe`) and Linux (`.tar.gz`) executables and attaches them to a GitHub release. There are no installers and no macOS builds. Planned: a built-in, opt-out update check that verifies a signed checksum and swaps the executable (see ROADMAP.md, Phase 5).
