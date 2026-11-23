import { call } from "./invoke";
import type { CalendarEvent, CalendarItem, ExternalCalendar, ImportSummary, SyncReport } from "./types";

export const localTimezone = () => Intl.DateTimeFormat().resolvedOptions().timeZone;

export const listCalendarRange = (from: string, to: string) =>
  call<CalendarItem[]>("list_calendar_range", { from, to });

export interface EventInput {
  title: string;
  description?: string;
  location?: string;
  all_day?: boolean;
  start_at: string;
  end_at: string;
  timezone: string;
  recurrence_rule?: string;
}
export const getEvent = (id: string) => call<CalendarEvent>("get_event", { id });
export const createEvent = (input: EventInput) => call<CalendarEvent>("create_event", { input });
export const updateEvent = (id: string, input: Partial<EventInput>) =>
  call<CalendarEvent>("update_event", { id, input });
export const deleteEvent = (id: string) => call<void>("delete_event", { id });
export const skipOccurrence = (eventId: string, originalStart: string) =>
  call<void>("skip_occurrence", { eventId, originalStart });
export const overrideOccurrence = (
  eventId: string,
  originalStart: string,
  input: { title?: string; description?: string; location?: string; start_at?: string; end_at?: string },
) => call<void>("override_occurrence", { eventId, originalStart, input });
export const restoreOccurrence = (eventId: string, originalStart: string) =>
  call<void>("restore_occurrence", { eventId, originalStart });
export const setTaskRecurrence = (taskId: string, rule: string | null, timezone: string) =>
  call<void>("set_task_recurrence", { taskId, rule, timezone });
export const setTaskOccurrence = (taskId: string, originalDue: string, status: "done" | "skipped" | "open") =>
  call<void>("set_task_occurrence", { taskId, originalDue, status });
export const exportIcsFile = (path: string) => call<void>("export_ics_file", { path });
export const importIcsFile = (path: string, timezone: string) =>
  call<ImportSummary>("import_ics_file", { path, timezone });

export const listExternalCalendars = () => call<ExternalCalendar[]>("list_external_calendars");
export const addExternalCalendar = (name: string, url: string) =>
  call<ExternalCalendar>("add_external_calendar", { name, url });
export const removeExternalCalendar = (id: string) => call<void>("remove_external_calendar", { id });
export const syncExternalCalendar = (id: string) => call<SyncReport>("sync_external_calendar", { id });
