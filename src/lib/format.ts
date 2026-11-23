/** Timestamps are stored in UTC and rendered in local time. */
export const fmtDateTime = (iso: string) =>
  new Date(iso).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
export const fmtDate = (iso: string) => new Date(iso).toLocaleDateString(undefined, { dateStyle: "medium" });
export const fmtTime = (iso: string) => new Date(iso).toLocaleTimeString(undefined, { timeStyle: "short" });

/** ISO (UTC) -> value for <input type="datetime-local"> in local time. */
export function toLocalInput(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}`;
}
/** <input type="datetime-local"> value (local) -> ISO UTC. Empty string stays empty. */
export const fromLocalInput = (v: string) => (v ? new Date(v).toISOString() : "");

export const PRIORITIES = ["None", "Low", "Medium", "High"] as const;
export const isOverdue = (due: string | null, done: boolean) => !!due && !done && new Date(due) < new Date();

export function fmtDuration(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const p = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${p(m)}:${p(sec)}` : `${p(m)}:${p(sec)}`;
}
export function fmtHours(ms: number): string {
  const m = Math.round(ms / 60000);
  return m < 60 ? `${m}m` : `${Math.floor(m / 60)}h ${m % 60}m`;
}
