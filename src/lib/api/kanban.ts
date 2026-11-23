import { call } from "./invoke";
import type {
  Board, BoardView, ChecklistItem, Column, ColumnKind, Project, Task, TaskTemplate, TemplateNode,
} from "./types";

export const listProjects = () => call<Project[]>("list_projects");
export const createProject = (name: string, description?: string, color?: string) =>
  call<Project>("create_project", { name, description, color });
export const updateProject = (id: string, input: { name?: string; description?: string; color?: string }) =>
  call<Project>("update_project", { id, input });
export const deleteProject = (id: string) => call<void>("delete_project", { id });

export const listBoards = (projectId: string) => call<Board[]>("list_boards", { projectId });
export const createBoard = (projectId: string, name: string) => call<Board>("create_board", { projectId, name });
export const renameBoard = (id: string, name: string) => call<Board>("rename_board", { id, name });
export const deleteBoard = (id: string) => call<void>("delete_board", { id });

export const createColumn = (boardId: string, name: string, kind: ColumnKind) =>
  call<Column>("create_column", { boardId, name, kind });
export const updateColumn = (id: string, name?: string, kind?: ColumnKind) =>
  call<Column>("update_column", { id, name, kind });
export const moveColumn = (id: string, position: number) => call<Column>("move_column", { id, position });
export const deleteColumn = (id: string) => call<void>("delete_column", { id });

export const boardView = (boardId: string) => call<BoardView>("board_view", { boardId });

export interface CreateTaskInput {
  column_id?: string;
  parent_id?: string;
  title: string;
  description?: string;
  priority?: number;
  due_at?: string;
  position?: number;
}
export interface UpdateTaskInput {
  title?: string;
  /** empty string clears */
  description?: string;
  priority?: number;
  /** empty string clears */
  due_at?: string;
}
export const createTask = (input: CreateTaskInput) => call<Task>("create_task", { input });
export const updateTask = (id: string, input: UpdateTaskInput) => call<Task>("update_task", { id, input });
export const moveTask = (id: string, columnId: string, position: number, force = false) =>
  call<Task>("move_task", { id, columnId, position, force });
export const setTaskCompleted = (id: string, completed: boolean, force = false) =>
  call<Task>("set_task_completed", { id, completed, force });
export const deleteTask = (id: string) => call<void>("delete_task", { id });

export const addDependency = (taskId: string, blockedById: string) =>
  call<void>("add_dependency", { taskId, blockedById });
export const removeDependency = (taskId: string, blockedById: string) =>
  call<void>("remove_dependency", { taskId, blockedById });

export const addChecklistItem = (taskId: string, text: string) =>
  call<ChecklistItem>("add_checklist_item", { taskId, text });
export const updateChecklistItem = (id: string, text?: string, done?: boolean) =>
  call<ChecklistItem>("update_checklist_item", { id, text, done });
export const deleteChecklistItem = (id: string) => call<void>("delete_checklist_item", { id });

export const listTemplates = () => call<TaskTemplate[]>("list_templates");
export const createTemplate = (name: string, tree: TemplateNode[], description?: string) =>
  call<TaskTemplate>("create_template", { name, description, tree });
export const deleteTemplate = (id: string) => call<void>("delete_template", { id });
export const instantiateTemplate = (templateId: string, columnId: string, parentId?: string) =>
  call<Task[]>("instantiate_template", { templateId, columnId, parentId });
export const saveTaskAsTemplate = (taskId: string, name: string) =>
  call<TaskTemplate>("save_task_as_template", { taskId, name });
