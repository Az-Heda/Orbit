export interface Setting {
  key: string;
  /** JSON-encoded value */
  value: string;
  updated_at: string;
}

export interface ActivityEntry {
  id: string;
  entity_type: string;
  entity_id: string;
  action: string;
  actor: "user" | "ai" | "automation";
  before_json: string | null;
  after_json: string | null;
  batch_id: string | null;
  created_at: string;
}

export interface AppError {
  kind: "validation" | "not_found" | "requires_confirmation" | "storage";
  message: string;
}

export interface Project {
  id: string;
  name: string;
  description: string | null;
  color: string | null;
  created_at: string;
  updated_at: string;
}

export interface Board {
  id: string;
  project_id: string;
  name: string;
  position: number;
}

export type ColumnKind = "backlog" | "active" | "done";

export interface Column {
  id: string;
  board_id: string;
  name: string;
  kind: ColumnKind;
  position: number;
}

export interface Task {
  id: string;
  column_id: string;
  parent_id: string | null;
  title: string;
  description: string | null;
  /** 0 none, 1 low, 2 medium, 3 high */
  priority: number;
  due_at: string | null;
  position: number;
  completed_at: string | null;
  created_at: string;
  updated_at: string;
  recurrence_rule: string | null;
  recurrence_tz: string | null;
}

export interface ChecklistItem {
  id: string;
  task_id: string;
  text: string;
  done: boolean;
  position: number;
}

export interface Dependency {
  task_id: string;
  blocked_by_id: string;
}

export interface BoardView {
  board: Board;
  columns: Column[];
  tasks: Task[];
  checklist_items: ChecklistItem[];
  dependencies: Dependency[];
}

export interface TaskTemplate {
  id: string;
  name: string;
  description: string | null;
  tree_json: string;
}

export interface TemplateNode {
  title: string;
  description?: string | null;
  priority?: number;
  due_offset_days?: number | null;
  children?: TemplateNode[];
  checklist?: string[];
}

export interface CalendarItem {
  key: string;
  kind: "event" | "task";
  source_id: string;
  title: string;
  description: string | null;
  location: string | null;
  start_at: string;
  end_at: string;
  all_day: boolean;
  timezone: string | null;
  recurring: boolean;
  occurrence_start: string | null;
  overridden: boolean;
  completed: boolean;
  priority: number;
  project_id: string | null;
  board_id: string | null;
}

export interface CalendarEvent {
  id: string;
  uid: string;
  title: string;
  description: string | null;
  location: string | null;
  all_day: boolean;
  start_at: string;
  end_at: string;
  timezone: string;
  recurrence_rule: string | null;
}

export interface ImportSummary {
  created: number;
  skipped_duplicates: number;
  warnings: string[];
}
