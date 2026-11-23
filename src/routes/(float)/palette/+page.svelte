<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { hideWindow, openCapture, openMain, toggleWidget } from "$lib/api/desktop";
  import { searchAll, type SearchHit } from "$lib/api/notes";
  import { toast } from "$lib/stores/toast.svelte";

  type Row = { id: string; label: string; hint: string; run: () => void | Promise<void> };

  let query = $state("");
  let hits = $state<SearchHit[]>([]);
  let sel = $state(0);
  let input = $state<HTMLInputElement>();
  let timer: ReturnType<typeof setTimeout> | undefined;
  let seq = 0;

  const go = async (path: string) => {
    await openMain(path);
    await hideWindow("palette");
  };
  const commands: Row[] = [
    { id: "c-capture", label: "Quick capture", hint: "Inbox", run: async () => { await hideWindow("palette"); await openCapture(); } },
    { id: "c-widget", label: "Toggle widget", hint: "Window", run: async () => { await toggleWidget(); await hideWindow("palette"); } },
    ...[["Overview", "/"], ["Inbox", "/inbox"], ["Projects", "/projects"], ["Calendar", "/calendar"], ["Notes", "/notes"], ["Timers", "/timers"], ["Stats", "/stats"], ["Automation", "/automation"], ["Settings", "/settings"]].map(
      ([label, path]): Row => ({ id: "go" + path, label: `Go to ${label}`, hint: "Navigate", run: () => go(path) }),
    ),
  ];
  const target = (h: SearchHit) =>
    h.kind === "task" || h.kind === "project" ? `/projects/${h.project_id}` : h.kind === "event" ? "/calendar" : h.kind === "note" ? `/notes?id=${h.id}` : "/inbox";

  const rows = $derived<Row[]>([
    ...commands.filter((c) => !query.trim() || c.label.toLowerCase().includes(query.trim().toLowerCase())).slice(0, 5),
    ...hits.map((h): Row => ({ id: h.kind + h.id, label: h.title, hint: h.kind, run: () => go(target(h)) })),
  ]);

  onMount(() => {
    input?.focus();
    const un = listen("summoned", () => {
      query = "";
      hits = [];
      sel = 0;
      input?.focus();
    });
    return () => void un.then((f) => f());
  });

  function onInput() {
    sel = 0;
    clearTimeout(timer);
    timer = setTimeout(async () => {
      const my = ++seq;
      try {
        const r = query.trim() ? await searchAll(query, undefined, 8) : [];
        if (my === seq) hits = r;
      } catch (e) {
        toast.error(e);
      }
    }, 120);
  }
  function key(e: KeyboardEvent) {
    if (e.key === "ArrowDown") (e.preventDefault(), (sel = Math.min(rows.length - 1, sel + 1)));
    else if (e.key === "ArrowUp") (e.preventDefault(), (sel = Math.max(0, sel - 1)));
    else if (e.key === "Enter") void rows[sel]?.run();
    else if (e.key === "Escape") void hideWindow("palette");
  }
</script>

<div class="card flex h-screen flex-col gap-2 border-primary bg-surface p-3 shadow-lg">
  <input
    bind:this={input}
    bind:value={query}
    oninput={onInput}
    onkeydown={key}
    class="input text-base"
    placeholder="Search everything or run a command…"
    role="combobox"
    aria-expanded="true"
    aria-controls="palette-list"
    aria-activedescendant={rows[sel] ? `row-${sel}` : undefined}
  />
  <ul id="palette-list" role="listbox" class="min-h-0 flex-1 space-y-1 overflow-auto">
    {#each rows as r, i (r.id)}
      <li id="row-{i}" role="option" aria-selected={i === sel}>
        <button class="flex w-full items-center justify-between rounded-md px-3 py-2 text-left text-sm data-[sel=true]:bg-primary/15" data-sel={i === sel} tabindex="-1" onclick={() => r.run()} onmouseenter={() => (sel = i)}>
          <span class="truncate">{r.label}</span><span class="chip">{r.hint}</span>
        </button>
      </li>
    {/each}
  </ul>
</div>
