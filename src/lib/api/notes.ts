import { call } from "./invoke";

export interface Note {
  id: string;
  title: string;
  body: string;
  kind: "note" | "journal";
  journal_date: string | null;
  created_at: string;
  updated_at: string;
}
export interface NoteLink {
  entity_type: "task" | "event" | "project";
  entity_id: string;
  title: string | null;
}
export type NoteDetail = Note & { links: NoteLink[] };

export interface SearchHit {
  kind: "task" | "event" | "note" | "project" | "inbox";
  id: string;
  title: string;
  snippet: string;
  score: number;
  project_id: string | null;
  board_id: string | null;
  date: string | null;
}

export const createNote = (title: string, body = "") => call<Note>("create_note", { title, body });
export const getNote = (id: string) => call<NoteDetail>("get_note", { id });
export const updateNote = (id: string, patch: { title?: string; body?: string }) =>
  call<Note>("update_note", { id, ...patch });
export const deleteNote = (id: string) => call<void>("delete_note", { id });
export const listNotes = (kind?: "note" | "journal") => call<Note[]>("list_notes", { kind, limit: 200 });
export const openJournal = (date: string) => call<Note>("open_journal", { date });
export const addNoteLink = (noteId: string, entityType: string, entityId: string) =>
  call<void>("add_note_link", { noteId, entityType, entityId });
export const removeNoteLink = (noteId: string, entityType: string, entityId: string) =>
  call<void>("remove_note_link", { noteId, entityType, entityId });
export const searchAll = (query: string, kinds?: string[], limit = 20) =>
  call<SearchHit[]>("search_all", { query, kinds, limit });
