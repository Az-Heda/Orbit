# Orbit user guide

Orbit is a local-first planner: everything lives in a database on your computer. It works fully offline and without any account.

## The basics

- **Projects and boards**: a project has kanban boards. Tasks can have subtasks, checklists, priorities, due dates and "blocked by" dependencies. Completing a parent with open subtasks, or moving a blocked task forward, warns you first.
- **Inbox**: `Ctrl+Alt+N` captures a thought from anywhere. Your original text is always kept; triage turns an item into a task or event.
- **Calendar**: events (with recurrence), tasks with due dates, `.ics` import/export, and read-only subscriptions to external calendar feeds (Settings).
- **Notes**: markdown notes linked to tasks, events or projects, searchable.
- **Timers**: Pomodoro and stopwatch. Time is computed from timestamps, so sleeping the computer does not break it.
- **Stats**: focus time, completed tasks and streaks.
- **Automation**: "when X, then A, then B..." rules; each can be disabled or edited. A rule can have up to 10 steps that run top to bottom; if one fails, the rest are skipped and the failure is recorded in the activity log (Settings, Recent activity).
- **Notification channels** (Settings): named places a rule can send a message to: a Windows notification, a Telegram chat (bot token + chat id) or a Discord channel (webhook URL). Each has a message template with placeholders such as `{{task.title}}` and `{{rule.name}}`, and a "Send test" button. In a rule, add the step "send a message to a channel" and pick the channel by name. Tokens and webhook URLs are stored in the Windows Credential Manager and are not shown again; delivery problems are logged and never stop Orbit. The assistant can draft rules that use a channel by name, but cannot see tokens or create channels.
- **Search**: `Ctrl+Alt+Space` opens the command palette over everything.

## Claude assistant (optional)

Add your own Anthropic API key in Settings (stored in the Windows Credential Manager, never shown again). Choose which categories (tasks, notes, focus data...) the assistant may read. Changes it suggests are shown as a preview and applied only when you confirm; each batch can be undone.

## Your data

- **Backups**: a daily automatic backup is kept (Settings > Data, default 7 kept). "Back up now" makes one that is never deleted automatically.
- **Export / import**: export writes your whole database to a `.db` file. Importing one (or restoring a backup) replaces your current data after the next restart; a safety backup of the current data is made first.

## Updates

Orbit checks for new versions once a day and asks before doing anything (Update / Later / Skip this version). You can turn the check off in Settings and use "Check now" manually. Your data is backed up before an update is installed.
