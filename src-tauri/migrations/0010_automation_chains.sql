-- Phase 6: chained automation actions and notification channels.

-- A rule now holds an ordered list of steps (JSON array of actions) instead of a single action.
-- The old CHECK on action_kind cannot be altered in SQLite, so the table is rebuilt; each existing
-- rule becomes a one-step chain.
CREATE TABLE automation_rules_v2 (
    id             TEXT PRIMARY KEY NOT NULL,
    name           TEXT NOT NULL,
    enabled        INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    trigger_kind   TEXT NOT NULL CHECK (trigger_kind IN
                     ('task_created', 'task_moved', 'task_completed', 'timer_ended', 'inbox_added', 'schedule')),
    trigger_config TEXT NOT NULL DEFAULT '{}',
    actions        TEXT NOT NULL DEFAULT '[]',
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    deleted_at     TEXT
) STRICT;

INSERT INTO automation_rules_v2
    (id, name, enabled, trigger_kind, trigger_config, actions, created_at, updated_at, deleted_at)
SELECT id, name, enabled, trigger_kind, trigger_config, '[' || action_config || ']',
       created_at, updated_at, deleted_at
FROM automation_rules;

DROP TABLE automation_rules;
ALTER TABLE automation_rules_v2 RENAME TO automation_rules;

-- Named notification channels. Secrets (Telegram bot token, Discord webhook URL) live in the OS
-- credential store keyed by the channel id, never in this table. `config` holds non-secret
-- settings only (the Telegram chat id).
CREATE TABLE notification_channels (
    id         TEXT PRIMARY KEY NOT NULL,
    name       TEXT NOT NULL,
    kind       TEXT NOT NULL CHECK (kind IN ('windows', 'telegram', 'discord')),
    config     TEXT NOT NULL DEFAULT '{}',
    template   TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT
) STRICT;
CREATE UNIQUE INDEX idx_notification_channels_name
    ON notification_channels (lower(name)) WHERE deleted_at IS NULL;
