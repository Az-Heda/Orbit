-- Timers store timestamps only; elapsed time is always derived from the clock.
--
-- A timer runs in segments. `accumulated_ms` holds time from finished segments,
-- `segment_started_at` is set while running (NULL while paused/stopped).
-- elapsed = accumulated_ms + (now - segment_started_at)  when running.

CREATE TABLE timers (
    id                 TEXT PRIMARY KEY NOT NULL,
    kind               TEXT NOT NULL CHECK (kind IN ('pomodoro', 'countdown', 'stopwatch')),
    label              TEXT,
    task_id            TEXT REFERENCES tasks (id),
    -- planned length of this timer (NULL for stopwatch)
    duration_ms        INTEGER CHECK (duration_ms IS NULL OR duration_ms > 0),
    -- pomodoro only: 'focus' or 'break' phase
    phase              TEXT NOT NULL DEFAULT 'focus' CHECK (phase IN ('focus', 'break')),
    state              TEXT NOT NULL CHECK (state IN ('running', 'paused', 'finished', 'cancelled')),
    started_at         TEXT NOT NULL,              -- first start, UTC
    segment_started_at TEXT,                       -- UTC, only while running
    accumulated_ms     INTEGER NOT NULL DEFAULT 0,
    ended_at           TEXT,
    created_at         TEXT NOT NULL,
    updated_at         TEXT NOT NULL
) STRICT;
CREATE INDEX idx_timers_state ON timers (state);

-- One row per completed focus period, linked to a task when there is one.
CREATE TABLE focus_sessions (
    id          TEXT PRIMARY KEY NOT NULL,
    timer_id    TEXT REFERENCES timers (id),
    task_id     TEXT REFERENCES tasks (id),
    started_at  TEXT NOT NULL,                     -- UTC
    ended_at    TEXT NOT NULL,                     -- UTC
    duration_ms INTEGER NOT NULL CHECK (duration_ms >= 0),
    created_at  TEXT NOT NULL
) STRICT;
CREATE INDEX idx_focus_started ON focus_sessions (started_at);
CREATE INDEX idx_focus_task ON focus_sessions (task_id);
