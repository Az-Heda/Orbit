-- Phase 2: reminders/automation dedupe keys and automation rules.

-- Idempotency keys for things that fire exactly once (reminders, scheduled rules).
-- A key like "reminder:<item key>:<lead>" survives restarts and sleep/resume, so a catch-up
-- pass after wake-up never double-fires.
CREATE TABLE fired_keys (
    key       TEXT PRIMARY KEY NOT NULL,
    fired_at  TEXT NOT NULL,
    missed    INTEGER NOT NULL DEFAULT 0 CHECK (missed IN (0, 1))
) STRICT;
CREATE INDEX idx_fired_at ON fired_keys (fired_at);

-- Closed set of triggers and actions; config is validated JSON (see domain/automation.rs).
CREATE TABLE automation_rules (
    id             TEXT PRIMARY KEY NOT NULL,
    name           TEXT NOT NULL,
    enabled        INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    trigger_kind   TEXT NOT NULL CHECK (trigger_kind IN
                     ('task_created', 'task_moved', 'task_completed', 'timer_ended', 'inbox_added', 'schedule')),
    trigger_config TEXT NOT NULL DEFAULT '{}',
    action_kind    TEXT NOT NULL CHECK (action_kind IN
                     ('create_task', 'create_note', 'move_task', 'start_timer', 'notify')),
    action_config  TEXT NOT NULL DEFAULT '{}',
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    deleted_at     TEXT
) STRICT;
