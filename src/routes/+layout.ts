// Tauri doesn't have a Node.js server to do proper SSR
// so we use adapter-static with a fallback to index.html to put the site in SPA mode
// See: https://svelte.dev/docs/kit/single-page-apps
// See: https://v2.tauri.app/start/frontend/sveltekit/ for more info
import { applyTheme, loadTheme } from "$lib/theme";

export const ssr = false;

/** Load and apply the saved theme before the first render so there is no flash. */
export async function load() {
  const theme = await loadTheme();
  applyTheme(theme);
  return { theme };
}
