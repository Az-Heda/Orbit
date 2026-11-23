import { getSetting, setSetting } from "$lib/api/settings";

export type ThemeMode = "system" | "light" | "dark";

export interface ThemeSettings {
  mode: ThemeMode;
  primary: string;
  secondary: string;
  /** 0-100 */
  glow: number;
  /** corner radius in px, 0-24 */
  radius: number;
}

export const DEFAULT_THEME: ThemeSettings = {
  mode: "system",
  primary: "#3b82f6",
  secondary: "#8b5cf6",
  glow: 50,
  radius: 10,
};

export const PRESETS = [
  { name: "Ion", primary: "#3b82f6", secondary: "#8b5cf6" },
  { name: "Neon", primary: "#00b8d9", secondary: "#e11d9a" },
  { name: "Solar", primary: "#f59e0b", secondary: "#ef4444" },
  { name: "Matrix", primary: "#16a34a", secondary: "#06b6d4" },
] as const;

const KEY = "theme";
const HEX = /^#[0-9a-f]{6}$/i;
const clamp = (v: unknown, min: number, max: number, fallback: number) =>
  typeof v === "number" && Number.isFinite(v) ? Math.min(max, Math.max(min, v)) : fallback;

function parse(raw: string | null | undefined): ThemeSettings | null {
  if (!raw) return null;
  try {
    let p = JSON.parse(raw);
    if (typeof p === "string") p = JSON.parse(p); // tolerate double-encoded values
    return {
      mode: p.mode === "light" || p.mode === "dark" ? p.mode : "system",
      primary: HEX.test(p.primary ?? "") ? p.primary : DEFAULT_THEME.primary,
      secondary: HEX.test(p.secondary ?? "") ? p.secondary : DEFAULT_THEME.secondary,
      glow: clamp(p.glow, 0, 100, DEFAULT_THEME.glow),
      radius: clamp(p.radius, 0, 24, DEFAULT_THEME.radius),
    };
  } catch {
    return null;
  }
}

export async function loadTheme(): Promise<ThemeSettings> {
  try {
    return parse((await getSetting(KEY))?.value) ?? DEFAULT_THEME;
  } catch (e) {
    console.error("Failed to load theme", e);
    return DEFAULT_THEME;
  }
}

export async function saveTheme(t: ThemeSettings): Promise<void> {
  try {
    await setSetting(KEY, t);
  } catch (e) {
    console.error("Failed to save theme", e);
  }
}

/** Black or white text, whichever reads better on the given background. */
function onColor(hex: string): string {
  const [r, g, b] = [1, 3, 5].map((i) => {
    const c = parseInt(hex.slice(i, i + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b > 0.4 ? "#0a0d1a" : "#ffffff";
}

export const systemPrefersDark = () => window.matchMedia("(prefers-color-scheme: dark)").matches;

export function applyTheme(t: ThemeSettings) {
  const root = document.documentElement;
  const dark = t.mode === "system" ? systemPrefersDark() : t.mode === "dark";
  root.dataset.theme = dark ? "dark" : "light";
  root.style.setProperty("--primary", t.primary);
  root.style.setProperty("--secondary", t.secondary);
  root.style.setProperty("--primary-contrast", onColor(t.primary));
  root.style.setProperty("--glow", String(t.glow / 100));
  root.style.setProperty("--radius", `${t.radius}px`);
}
