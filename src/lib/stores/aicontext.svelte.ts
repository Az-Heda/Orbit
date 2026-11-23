/**
 * What the user was looking at, so "this task" / "here" resolve in the assistant. Pages set it;
 * the assistant page itself never overwrites it. Selections belong to the page that set them and
 * are dropped when the user navigates to another page. Ids are validated again in Rust.
 */
class AiContextState {
  view = $state<string | null>(null);
  projectId = $state<string | null>(null);
  boardId = $state<string | null>(null);
  taskId = $state<string | null>(null);
  eventId = $state<string | null>(null);
  noteId = $state<string | null>(null);
  /** Human-readable labels for the chips. */
  labels = $state<Record<string, string>>({});

  navigate(path: string) {
    if (path.startsWith("/assistant")) return;
    if (path !== this.view) {
      this.projectId = this.boardId = this.taskId = this.eventId = this.noteId = null;
      this.labels = {};
    }
    this.view = path;
  }
  set(field: "projectId" | "boardId" | "taskId" | "eventId" | "noteId", id: string | null, label?: string) {
    this[field] = id;
    if (id && label) this.labels[field] = label;
  }
  clear(field: "projectId" | "boardId" | "taskId" | "eventId" | "noteId") {
    this[field] = null;
  }

  toPayload() {
    return {
      view: this.view,
      timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
      project_id: this.projectId,
      board_id: this.boardId,
      task_id: this.taskId,
      event_id: this.eventId,
      note_id: this.noteId,
    };
  }
}
export const aiContext = new AiContextState();
