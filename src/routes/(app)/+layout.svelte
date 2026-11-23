<script lang="ts">
  import { page } from "$app/state";
  import { goto } from "$app/navigation";
  import ConfirmDialog from "$lib/components/ConfirmDialog.svelte";
  import Toast from "$lib/components/Toast.svelte";
  import UpdateDialog from "$lib/components/UpdateDialog.svelte";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { inboxCount } from "$lib/api/inbox";
  import { aiContext } from "$lib/stores/aicontext.svelte";

  let { children } = $props();
  let inbox = $state(0);

  const nav = [
    { href: "/", label: "Overview", icon: '<rect x="3" y="3" width="7" height="9"/><rect x="14" y="3" width="7" height="5"/><rect x="14" y="12" width="7" height="9"/><rect x="3" y="16" width="7" height="5"/>' },
    { href: "/projects", label: "Projects", icon: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>' },
    { href: "/assistant", label: "Assistant", icon: '<path d="M12 3l1.8 4.7L18.5 9.5l-4.7 1.8L12 16l-1.8-4.7L5.5 9.5l4.7-1.8z"/><path d="M19 15l.8 2.2L22 18l-2.2.8L19 21l-.8-2.2L16 18l2.2-.8z"/>' },
    { href: "/inbox", label: "Inbox", icon: '<path d="M22 12h-6l-2 3h-4l-2-3H2"/><path d="M5.5 5h13L22 12v6a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2v-6z"/>' },
    { href: "/calendar", label: "Calendar", icon: '<rect x="3" y="4" width="18" height="18" rx="2"/><path d="M16 2v4M8 2v4M3 10h18"/>' },
    { href: "/notes", label: "Notes", icon: '<path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z"/><path d="M14 3v6h6M8 13h8M8 17h5"/>' },
    { href: "/timers", label: "Timers", icon: '<circle cx="12" cy="13" r="8"/><path d="M12 9v4l2 2M9 2h6"/>' },
    { href: "/stats", label: "Stats", icon: '<path d="M3 3v18h18"/><path d="M7 15v3M12 9v9M17 5v13"/>' },
    { href: "/automation", label: "Automation", icon: '<path d="M13 2 3 14h9l-1 8 10-12h-9z"/>' },
    { href: "/search", label: "Search", icon: '<circle cx="11" cy="11" r="7"/><path d="m21 21-4.3-4.3"/>' },
    { href: "/settings", label: "Settings", icon: '<circle cx="12" cy="12" r="3"/><path d="M12 2v3M12 19v3M4.9 4.9l2.1 2.1M17 17l2.1 2.1M2 12h3M19 12h3M4.9 19.1 7 17M17 7l2.1-2.1"/>' },
  ];
  const refreshInbox = () => inboxCount().then((n) => (inbox = n)).catch(() => {});
  onMount(() => {
    refreshInbox();
    const subs = [
      listen("inbox-changed", refreshInbox),
      listen("domain-event", refreshInbox),
      // The palette/tray ask the main window to open a page.
      listen<string>("navigate", (e) => goto(e.payload)),
    ];
    return () => subs.forEach((u) => void u.then((f) => f()));
  });
  function onKey(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
      e.preventDefault();
      goto("/search");
    }
  }
  // Pre-effect: must run before child pages record their selection for the new route.
  $effect.pre(() => aiContext.navigate(page.url.pathname));
  const active = (href: string) =>
    href === "/" ? page.url.pathname === "/" : page.url.pathname.startsWith(href);
</script>

<svelte:window onkeydown={onKey} />

<div class="flex h-screen">
  <nav class="flex w-60 shrink-0 flex-col gap-1 border-r border-border bg-surface px-3 py-6" aria-label="Main">
    <div class="mb-6 flex items-center gap-3 px-2">
      <span class="brand-mark" aria-hidden="true"></span>
      <div>
        <div class="font-display text-xl uppercase tracking-[0.12em]">Orbit</div>
        <div class="font-mono text-sm text-muted">productivity</div>
      </div>
    </div>
    {#each nav as item (item.href)}
      <a href={item.href} class="nav-item" aria-current={active(item.href) ? "page" : undefined}>
        <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">{@html item.icon}</svg>
        <span>{item.label}</span>
        {#if item.href === "/inbox" && inbox > 0}<span class="chip ml-auto" aria-label="{inbox} new">{inbox}</span>{/if}
      </a>
    {/each}
  </nav>
  <main class="grid-bg min-w-0 flex-1 overflow-auto">
    {@render children()}
  </main>
</div>

<ConfirmDialog />
<Toast />
<UpdateDialog />
