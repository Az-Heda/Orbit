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

- [ ] Claude client (`reqwest` + `serde`), streaming
- [ ] API key storage in Windows Credential Manager
- [ ] Tool registry with JSON schemas and validation
- [ ] Read tools: `get_schedule`, `get_tasks`
- [ ] Write tools: `create_event`, `update_event`, `create_task`, `move_task`, `delete_task`, `start_timer`
- [ ] Confirmation previews / diffs; mandatory confirmation for delete and bulk actions
- [ ] Contextual awareness (current board / selected task / current date and timezone)
- [ ] Chat UI + persisted conversation history
- [ ] Configurable AI context sharing (privacy settings)
- [ ] Undo of AI actions via `activity_log`
- [ ] Note tools: `search_notes`, `get_note`, `create_note` (respecting context-sharing settings)
- [ ] Inbox tools: `get_inbox`, `triage_inbox_item` (AI files captured items as tasks/events, with preview)
- [ ] Task tools: `add_dependency`, `create_subtask`, `create_task_from_template`; planning respects dependencies
- [ ] Search tool: `search_all`
- [ ] AI memory: editable memory list, `remember` / `forget` tools with confirmation, injected into context with a size cap
- [ ] AI can draft automation rules (user confirms before enabling)

## Phase 4: Intelligence

- [ ] Natural-language scheduling ("block 2 hours tomorrow for studying")
- [ ] AI daily planning from events, tasks, priorities and deadlines
- [ ] Schedule optimization (find free slots for unfinished tasks)
- [ ] Task breakdown into subtasks
- [ ] Smart organization (e.g. move all Rust-related tasks into the Rust project)
- [ ] Daily summary of completed/unfinished work
- [ ] Context-aware "what should I work on next?" suggestions
- [ ] Automatic association of focus sessions with tasks
- [ ] Read-only Google/Outlook calendar sync (opt-in, OAuth tokens in Credential Manager)
- [ ] AI answers from notes ("what did I decide about auth?")

## Phase 5: Polish and release

- [ ] Automatic local backups
- [ ] Export / import data
- [ ] FTS5 search across tasks, projects and notes
- [ ] Windows installer
- [ ] Auto-update
- [ ] Documentation and onboarding

## MVP definition

Phases 0 to 3 plus backups and export/import from Phase 5.

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

Accepted (already placed in the phases above): quick capture inbox, recurring tasks/events with smart reminders, calendar interop (`.ics` then read-only external sync), notes and journal, task depth (subtasks, dependencies, templates), stats dashboard, customizable widget, AI memory, automation rules, global search.

Parked: MCP server, local LLM, review ritual, voice input, history browser, proactive nudges.

Declined: automatic time tracking from the active Windows window, habit tracker, energy/estimate-aware scheduling, focus mode.
