<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { localTimezone } from "$lib/api/calendar";
  import * as api from "$lib/api/inbox";
  import ColumnPicker from "$lib/components/ColumnPicker.svelte";
  import { fmtDateTime, fromLocalInput, toLocalInput } from "$lib/format";
  import { toast } from "$lib/stores/toast.svelte";

  type Status = api.InboxItem["status"];
  let tab = $state<Status>("new");
  let items = $state<api.InboxItem[]>([]);
  let text = $state("");
  let busy = $state(false);
  let open = $state<{ id: string; mode: "task" | "event" } | null>(null);
  let title = $state("");
  let due = $state("");
  let column = $state("");
  let start = $state("");
  let end = $state("");

  async function load() {
    try {
      items = await api.listInbox(tab);
    } catch (e) {
      toast.error(e);
    }
  }
  onMount(() => {
    load();
    const un = listen("inbox-changed", load);
    return () => void un.then((f) => f());
  });
  const setTab = (t: Status) => {
    tab = t;
    open = null;
    load();
  };

  async function add(e: Event) {
    e.preventDefault();
    if (!text.trim()) return;
    try {
      await api.captureInbox(text);
      text = "";
      await load();
    } catch (err) {
      toast.error(err);
    }
  }
  function expand(it: api.InboxItem, mode: "task" | "event") {
    open = { id: it.id, mode };
    title = it.raw_text.split("\n").find((l) => l.trim())?.trim().slice(0, 200) ?? "";
    due = "";
    const s = new Date();
    s.setMinutes(0, 0, 0);
    s.setHours(s.getHours() + 1);
    start = toLocalInput(s.toISOString());
    end = toLocalInput(new Date(+s + 3_600_000).toISOString());
  }
  async function file() {
    if (!open || busy) return;
    busy = true;
    try {
      if (open.mode === "task") await api.triageToTask(open.id, column, title, due ? fromLocalInput(due) : undefined);
      else await api.triageToEvent(open.id, fromLocalInput(start), fromLocalInput(end), localTimezone(), title);
      open = null;
      await load();
    } catch (e) {
      toast.error(e);
    } finally {
      busy = false;
    }
  }
  async function dismiss(id: string) {
    try {
      await api.dismissInbox(id);
      await load();
    } catch (e) {
      toast.error(e);
    }
  }
</script>

<div class="mx-auto max-w-3xl space-y-6 p-8">
  <header>
    <h1 class="text-3xl">Inbox</h1>
    <p class="mt-2 text-muted">Capture now, decide later. Press Ctrl+Alt+N anywhere to add something.</p>
  </header>

  <form class="flex gap-2" onsubmit={add}>
    <input class="input flex-1" placeholder="Capture anything…" aria-label="Capture" bind:value={text} />
    <button class="btn btn-primary" disabled={!text.trim()}>Add</button>
  </form>

  <div class="seg-group" role="group" aria-label="Status">
    {#each ["new", "triaged", "dismissed"] as s}
      <button class="seg capitalize" aria-pressed={tab === s} onclick={() => setTab(s as Status)}>{s}</button>
    {/each}
  </div>

  <ul class="space-y-3">
    {#each items as it (it.id)}
      <li class="card space-y-3 p-4">
        <p class="whitespace-pre-wrap">{it.raw_text}</p>
        <p class="text-xs text-muted">
          {fmtDateTime(it.created_at)}{#if it.linked_entity_type} · filed as {it.linked_entity_type}{/if}
        </p>
        {#if it.status === "new"}
          <div class="flex flex-wrap gap-2">
            <button class="btn" onclick={() => expand(it, "task")}>To task…</button>
            <button class="btn" onclick={() => expand(it, "event")}>To event…</button>
            <button class="btn btn-danger" onclick={() => dismiss(it.id)}>Dismiss</button>
          </div>
        {/if}
        {#if open?.id === it.id}
          <div class="space-y-3 border-t border-border pt-3 text-sm">
            <label class="block"><span class="text-muted">Title</span><input class="input mt-1" bind:value={title} /></label>
            {#if open.mode === "task"}
              <ColumnPicker bind:value={column} />
              <label class="block"><span class="text-muted">Due (optional)</span><input class="input mt-1 w-auto" type="datetime-local" bind:value={due} /></label>
            {:else}
              <div class="flex gap-3">
                <label><span class="text-muted">Start</span><input class="input mt-1" type="datetime-local" bind:value={start} /></label>
                <label><span class="text-muted">End</span><input class="input mt-1" type="datetime-local" bind:value={end} /></label>
              </div>
            {/if}
            <div class="flex gap-2">
              <button class="btn btn-primary" onclick={file} disabled={busy || (open.mode === "task" && !column)}>File it</button>
              <button class="btn" onclick={() => (open = null)}>Cancel</button>
            </div>
          </div>
        {/if}
      </li>
    {:else}
      <li class="text-muted">{tab === "new" ? "Inbox zero. Nice." : "Nothing here."}</li>
    {/each}
  </ul>
</div>
