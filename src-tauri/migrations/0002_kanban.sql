-- Kanban: projects, boards, columns, tasks, checklists, dependencies, templates.
-- UUID text keys, UTC RFC 3339 timestamps, soft delete via deleted_at.

CREATE TABLE projects (
    id          TEXT PRIMARY KEY NOT NULL,
    name        TEXT NOT NULL,
    description TEXT,
    color       TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    deleted_at  TEXT
) STRICT;

CREATE TABLE boards (
    id         TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects (id),
    name       TEXT NOT NULL,
    position   INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT
) STRICT;
CREATE INDEX idx_boards_project ON boards (project_id);

CREATE TABLE board_columns (
    id         TEXT PRIMARY KEY NOT NULL,
    board_id   TEXT NOT NULL REFERENCES boards (id),
    name       TEXT NOT NULL,
    kind       TEXT NOT NULL CHECK (kind IN ('backlog', 'active', 'done')),
    position   INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT
) STRICT;
CREATE INDEX idx_columns_board ON board_columns (board_id);

CREATE TABLE tasks (
    id           TEXT PRIMARY KEY NOT NULL,
    column_id    TEXT NOT NULL REFERENCES board_columns (id),
    parent_id    TEXT REFERENCES tasks (id),
    title        TEXT NOT NULL,
    description  TEXT,
    priority     INTEGER NOT NULL DEFAULT 0 CHECK (priority BETWEEN 0 AND 3),
    due_at       TEXT,
    position     INTEGER NOT NULL,
    completed_at TEXT,
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL,
    deleted_at   TEXT
) STRICT;
CREATE INDEX idx_tasks_column ON tasks (column_id);
CREATE INDEX idx_tasks_parent ON tasks (parent_id);
CREATE INDEX idx_tasks_due ON tasks (due_at) WHERE due_at IS NOT NULL;

CREATE TABLE checklist_items (
    id         TEXT PRIMARY KEY NOT NULL,
    task_id    TEXT NOT NULL REFERENCES tasks (id),
    text       TEXT NOT NULL,
    done       INTEGER NOT NULL DEFAULT 0 CHECK (done IN (0, 1)),
    position   INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;
CREATE INDEX idx_checklist_task ON checklist_items (task_id);

-- task_id is blocked by blocked_by_id.
CREATE TABLE task_dependencies (
    task_id       TEXT NOT NULL REFERENCES tasks (id),
    blocked_by_id TEXT NOT NULL REFERENCES tasks (id),
    created_at    TEXT NOT NULL,
    PRIMARY KEY (task_id, blocked_by_id),
    CHECK (task_id <> blocked_by_id)
) STRICT;
CREATE INDEX idx_deps_blocker ON task_dependencies (blocked_by_id);

-- Templates are structured task trees stored as JSON.
CREATE TABLE task_templates (
    id          TEXT PRIMARY KEY NOT NULL,
    name        TEXT NOT NULL,
    description TEXT,
    tree_json   TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    deleted_at  TEXT
) STRICT;

INSERT INTO task_templates (id, name, description, tree_json, created_at, updated_at) VALUES (
    '00000000-0000-4000-8000-000000000001',
    'New project',
    'Standard kickoff checklist for a new project',
    '[{"title":"Define scope and goals","priority":3,"children":[],"checklist":["Write a one-paragraph goal","List what is out of scope"]},{"title":"Break down the work","priority":2,"children":[],"checklist":[]},{"title":"Set milestones and deadlines","priority":2,"due_offset_days":14,"children":[],"checklist":[]},{"title":"Review progress","priority":1,"due_offset_days":7,"children":[],"checklist":[]}]',
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
);
