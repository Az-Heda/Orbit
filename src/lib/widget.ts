import { getSetting, setSetting } from "$lib/api/settings";

export interface WidgetConfig {
  blocks: { clock: boolean; nextEvent: boolean; activeTask: boolean; timer: boolean; inbox: boolean; quickInput: boolean };
  /** "inherit" follows the app theme */
  mode: "inherit" | "light" | "dark";
  /** override accent colour, or null to use the app's primary */
  accent: string | null;
  /** 0.3 - 1 */
  opacity: number;
  clickThrough: boolean;
}

export const DEFAULT_WIDGET: WidgetConfig = {
  blocks: { clock: true, nextEvent: true, activeTask: true, timer: true, inbox: true, quickInput: true },
  mode: "inherit",
  accent: null,
  opacity: 0.92,
  clickThrough: false,
};

export const BLOCK_LABELS: Record<keyof WidgetConfig["blocks"], string> = {
  clock: "Clock",
  nextEvent: "Next event",
  activeTask: "Active task",
  timer: "Timer",
  inbox: "Inbox count",
  quickInput: "Quick input",
};

export function parseWidget(raw: unknown): WidgetConfig {
  const p = (typeof raw === "string" ? safeJson(raw) : raw) as Partial<WidgetConfig> | null;
  const b = (p?.blocks ?? {}) as Partial<WidgetConfig["blocks"]>;
  const opacity = typeof p?.opacity === "number" ? Math.min(1, Math.max(0.3, p.opacity)) : DEFAULT_WIDGET.opacity;
  return {
    blocks: {
      clock: b.clock ?? true,
      nextEvent: b.nextEvent ?? true,
      activeTask: b.activeTask ?? true,
      timer: b.timer ?? true,
      inbox: b.inbox ?? true,
      quickInput: b.quickInput ?? true,
    },
    mode: p?.mode === "light" || p?.mode === "dark" ? p.mode : "inherit",
    accent: typeof p?.accent === "string" && /^#[0-9a-f]{6}$/i.test(p.accent) ? p.accent : null,
    opacity,
    clickThrough: p?.clickThrough === true,
  };
}
function safeJson(s: string): unknown {
  try {
    return JSON.parse(s);
  } catch {
    return null;
  }
}

export async function loadWidget(): Promise<WidgetConfig> {
  try {
    return parseWidget((await getSetting("widget"))?.value);
  } catch {
    return DEFAULT_WIDGET;
  }
}
export const saveWidget = (c: WidgetConfig) => setSetting("widget", c);
