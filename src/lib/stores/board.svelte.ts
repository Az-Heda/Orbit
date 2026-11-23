import * as api from "$lib/api/kanban";
import { setTaskRecurrence } from "$lib/api/calendar";
import type { BoardView, ChecklistItem, Column, Task } from "$lib/api/types";
import { withConfirm } from "./confirm.svelte";
import { toast } from "./toast.svelte";

/** Board state + actions. Business rules live in the Rust services; this only orchestrates. */
export class BoardStore {
  view = $state<BoardView | null>(null);
  loading = $state(false);

  columns = $derived(this.view?.columns ?? []);
  allTasks = $derived(this.view?.tasks ?? []);

  rootTasks(columnId: string): Task[] {
    return this.allTasks.filter((t) => t.column_id === columnId && t.parent_id === null);
  }
  subtasks(parentId: string): Task[] {
    return this.allTasks.filter((t) => t.parent_id === parentId);
  }
  checklist(taskId: string): ChecklistItem[] {
    return (this.view?.checklist_items ?? []).filter((i) => i.task_id === taskId);
  }
  task(id: string | null): Task | undefined {
    return id ? this.allTasks.find((t) => t.id === id) : undefined;
  }
  /** Tasks that block `taskId` and are not completed yet. */
  openBlockers(taskId: string): Task[] {
    const ids = (this.view?.dependencies ?? []).filter((d) => d.task_id === taskId).map((d) => d.blocked_by_id);
    return ids.map((id) => this.task(id)).filter((t): t is Task => !!t && !t.completed_at);
  }
  blockerIds(taskId: string): string[] {
    return (this.view?.dependencies ?? []).filter((d) => d.task_id === taskId).map((d) => d.blocked_by_id);
  }
  column(id: string): Column | undefined {
    return this.columns.find((c) => c.id === id);
  }

  private boardId: string | null = null;

  async load(boardId: string) {
    this.boardId = boardId;
    this.loading = true;
    try {
      this.view = await api.boardView(boardId);
    } catch (e) {
      toast.error(e);
    } finally {
      this.loading = false;
    }
  }

  /** Run a mutation, then refresh. Errors surface as toasts. */
  async run<T>(action: () => Promise<T>): Promise<T | undefined> {
    try {
      const r = await action();
      if (this.boardId) this.view = await api.boardView(this.boardId);
      return r;
    } catch (e) {
      toast.error(e);
      return undefined;
    }
  }

  addTask(columnId: string, title: string) {
    return this.run(() => api.createTask({ column_id: columnId, title }));
  }
  addSubtask(parentId: string, title: string) {
    return this.run(() => api.createTask({ parent_id: parentId, title }));
  }
  update(id: string, input: api.UpdateTaskInput) {
    return this.run(() => api.updateTask(id, input));
  }
  move(id: string, columnId: string, position: number) {
    return this.run(() => withConfirm((force) => api.moveTask(id, columnId, position, force)));
  }
  setCompleted(id: string, completed: boolean) {
    return this.run(() => withConfirm((force) => api.setTaskCompleted(id, completed, force)));
  }
  remove(id: string) {
    return this.run(() => api.deleteTask(id));
  }
  addDependency(taskId: string, blockedById: string) {
    return this.run(() => api.addDependency(taskId, blockedById));
  }
  removeDependency(taskId: string, blockedById: string) {
    return this.run(() => api.removeDependency(taskId, blockedById));
  }
  addChecklist(taskId: string, text: string) {
    return this.run(() => api.addChecklistItem(taskId, text));
  }
  toggleChecklist(item: ChecklistItem) {
    return this.run(() => api.updateChecklistItem(item.id, undefined, !item.done));
  }
  removeChecklist(id: string) {
    return this.run(() => api.deleteChecklistItem(id));
  }
  setRecurrence(taskId: string, rule: string | null, timezone: string) {
    return this.run(() => setTaskRecurrence(taskId, rule, timezone));
  }
  addColumn(name: string, kind: Column["kind"]) {
    return this.run(() => api.createColumn(this.boardId!, name, kind));
  }
  renameColumn(id: string, name: string) {
    return this.run(() => api.updateColumn(id, name));
  }
  removeColumn(id: string) {
    return this.run(() => api.deleteColumn(id));
  }
  instantiateTemplate(templateId: string, columnId: string, parentId?: string) {
    return this.run(() => api.instantiateTemplate(templateId, columnId, parentId));
  }
  saveAsTemplate(taskId: string, name: string) {
    return this.run(() => api.saveTaskAsTemplate(taskId, name));
  }
}
