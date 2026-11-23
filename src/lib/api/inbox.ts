import { call } from "./invoke";
import type { CalendarEvent, Task } from "./types";

export interface InboxItem {
  id: string;
  raw_text: string;
  status: "new" | "triaged" | "dismissed";
  linked_entity_type: "task" | "event" | null;
  linked_entity_id: string | null;
  created_at: string;
  updated_at: string;
}

export const captureInbox = (text: string) => call<InboxItem>("capture_inbox", { text });
export const listInbox = (status?: InboxItem["status"]) => call<InboxItem[]>("list_inbox", { status, limit: 200 });
export const inboxCount = () => call<number>("inbox_count");
export const triageToTask = (id: string, columnId: string, title?: string, dueAt?: string) =>
  call<{ item: InboxItem; task: Task }>("triage_inbox_to_task", { id, columnId, title, dueAt });
export const triageToEvent = (id: string, startAt: string, endAt: string, timezone: string, title?: string) =>
  call<{ item: InboxItem; event: CalendarEvent }>("triage_inbox_to_event", { id, title, startAt, endAt, timezone });
export const dismissInbox = (id: string) => call<InboxItem>("dismiss_inbox", { id });
