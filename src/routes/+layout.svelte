<script lang="ts">
  import "../app.css";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { theme } from "$lib/stores/theme.svelte";
  import type { ThemeSettings } from "$lib/theme";

  let { children, data } = $props();
  // svelte-ignore state_referenced_locally
  theme.init(data.theme);

  // Every window (main, widget, palette, capture) stays in sync with theme changes.
  onMount(() => {
    const un = listen<ThemeSettings>("theme-changed", (e) => theme.applyRemote(e.payload));
    return () => void un.then((f) => f());
  });
</script>

{@render children()}
