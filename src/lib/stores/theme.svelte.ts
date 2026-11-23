import { applyTheme, DEFAULT_THEME, saveTheme, type ThemeSettings } from "$lib/theme";

class ThemeStore {
  value = $state<ThemeSettings>({ ...DEFAULT_THEME });
  private timer: ReturnType<typeof setTimeout> | undefined;

  init(initial: ThemeSettings) {
    this.value = initial;
    applyTheme(initial);
    // Follow OS changes while in "system" mode.
    window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => {
      if (this.value.mode === "system") applyTheme(this.value);
    });
  }

  update(patch: Partial<ThemeSettings>) {
    this.value = { ...this.value, ...patch };
    applyTheme(this.value);
    // Debounced so dragging a slider doesn't spam writes.
    clearTimeout(this.timer);
    this.timer = setTimeout(() => void saveTheme(this.value), 300);
  }

  reset() {
    this.update({ ...DEFAULT_THEME });
  }
}
export const theme = new ThemeStore();
