/** Pure date helpers for calendar views. All math is in the user's local timezone. */

export type View = "day" | "week" | "month";

export const startOfDay = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate());
export const addDays = (d: Date, n: number) => new Date(d.getFullYear(), d.getMonth(), d.getDate() + n);
/** Monday-based week start. */
export const startOfWeek = (d: Date) => addDays(startOfDay(d), -((d.getDay() + 6) % 7));
export const startOfMonth = (d: Date) => new Date(d.getFullYear(), d.getMonth(), 1);
export const sameDay = (a: Date, b: Date) => a.toDateString() === b.toDateString();

/** The half-open [from, to) range a view needs to load. */
export function viewRange(view: View, anchor: Date): [Date, Date] {
  if (view === "day") return [startOfDay(anchor), addDays(startOfDay(anchor), 1)];
  if (view === "week") {
    const s = startOfWeek(anchor);
    return [s, addDays(s, 7)];
  }
  const s = startOfWeek(startOfMonth(anchor));
  return [s, addDays(s, 42)];
}

export function days(from: Date, to: Date): Date[] {
  const out: Date[] = [];
  for (let d = from; d < to; d = addDays(d, 1)) out.push(d);
  return out;
}

export function shift(view: View, anchor: Date, dir: -1 | 1): Date {
  if (view === "day") return addDays(anchor, dir);
  if (view === "week") return addDays(anchor, 7 * dir);
  return new Date(anchor.getFullYear(), anchor.getMonth() + dir, 1);
}

/** Does an item overlap the local day `day`? End is exclusive (zero-length items match their start). */
export function overlapsDay(item: { start_at: string; end_at: string }, day: Date): boolean {
  const s = new Date(item.start_at).getTime();
  const e = new Date(item.end_at).getTime();
  const d0 = day.getTime();
  const d1 = addDays(day, 1).getTime();
  return s < d1 && (e > d0 || (e === s && s >= d0));
}

// ---- recurrence rule builder (simple presets) ----

export type Freq = "" | "DAILY" | "WEEKLY" | "MONTHLY" | "YEARLY";
export interface RuleParts {
  freq: Freq;
  interval: number;
  end: "never" | "count" | "until";
  count: number;
  until: string; // yyyy-mm-dd (local)
}
export const defaultParts = (): RuleParts => ({ freq: "", interval: 1, end: "never", count: 10, until: "" });

export function buildRule(p: RuleParts): string {
  if (!p.freq) return "";
  let r = `FREQ=${p.freq}`;
  if (p.interval > 1) r += `;INTERVAL=${p.interval}`;
  if (p.end === "count") r += `;COUNT=${Math.max(1, p.count)}`;
  if (p.end === "until" && p.until) {
    // inclusive through the end of that local day
    const d = new Date(`${p.until}T23:59:59`);
    r += `;UNTIL=${d.toISOString().replace(/[-:]/g, "").replace(/\.\d{3}/, "")}`;
  }
  return r;
}

/** Parse rules produced by buildRule; returns null for anything more complex ("custom"). */
export function parseRule(rule: string | null): RuleParts | null {
  const p = defaultParts();
  if (!rule) return p;
  for (const part of rule.split(";")) {
    const [k, v] = part.split("=");
    if (k === "FREQ" && ["DAILY", "WEEKLY", "MONTHLY", "YEARLY"].includes(v)) p.freq = v as Freq;
    else if (k === "INTERVAL") p.interval = Number(v) || 1;
    else if (k === "COUNT") (p.end = "count"), (p.count = Number(v) || 1);
    else if (k === "UNTIL") {
      const m = /^(\d{4})(\d{2})(\d{2})T(\d{2})(\d{2})(\d{2})Z$/.exec(v);
      if (!m) return null;
      const d = new Date(Date.UTC(+m[1], +m[2] - 1, +m[3], +m[4], +m[5], +m[6]));
      const pad = (n: number) => String(n).padStart(2, "0");
      p.end = "until";
      p.until = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
    } else return null;
  }
  return p.freq ? p : null;
}

export function describeRule(rule: string | null): string {
  const p = parseRule(rule);
  if (!rule) return "";
  if (!p) return rule;
  const unit = { DAILY: "day", WEEKLY: "week", MONTHLY: "month", YEARLY: "year" }[p.freq as Exclude<Freq, "">];
  let s = p.interval > 1 ? `Every ${p.interval} ${unit}s` : `Every ${unit}`;
  if (p.end === "count") s += `, ${p.count} times`;
  if (p.end === "until") s += `, until ${p.until}`;
  return s;
}
