<script lang="ts">
  import { onMount } from "svelte";
  import * as api from "$lib/api/notes";
  import { renderMarkdown } from "$lib/markdown";
  import { confirmState } from "$lib/stores/confirm.svelte";
  import { toast } from "$lib/stores/toast.svelte";

  let notes = $state<api.Note[]>([]);
  let current = $state<api.NoteDetail | null>(null);
  let title = $state("");
  let body = $state("");
  let preview = $state(false);
  let saved = $state(true);
  let linkQuery = $state("");
  let linkHits = $state<api.SearchHit[]>([]);
  let timer: ReturnType<typeof setTimeout> | undefined;

  const localDate = () => {
    const d = new Date();
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
  };
  const guard = async <T,>(f: () => Promise<T>) => {
    try {
      return await f();
    } catch (e) {
      toast.error(e);
    }
  };

  async function refresh() {
    notes = (await guard(() => api.listNotes())) ?? notes;
  }
  async function select(id: string) {
    await flush();
    const n = await guard(() => api.getNote(id));
    if (!n) return;
    current = n;
    title = n.title;
    body = n.body;
    saved = true;
    preview = false;
  }
  async function flush() {
    clearTimeout(timer);
    if (current && !saved) {
      const id = current.id;
      const n = await guard(() => api.updateNote(id, { title: title.trim() || current!.title, body }));
      if (n) saved = true;
      await refresh();
    }
  }
  function edited() {
    saved = false;
    clearTimeout(timer);
    timer = setTimeout(flush, 800);
  }
  async function create() {
    await flush();
    const n = await guard(() => api.createNote("Untitled note"));
    if (n) {
      await refresh();
      await select(n.id);
    }
  }
  async function journal() {
    await flush();
    const n = await guard(() => api.openJournal(localDate()));
    if (n) {
      await refresh();
      await select(n.id);
    }
  }
  async function remove() {
    if (!current || !(await confirmState.ask(`Delete "${current.title}"?`, "Delete"))) return;
    await guard(() => api.deleteNote(current!.id));
    current = null;
    await refresh();
  }
  async function searchLinks() {
    const q = linkQuery.trim();
    linkHits = q ? ((await guard(() => api.searchAll(q, ["task", "event", "project"], 6))) ?? []) : [];
  }
  async function link(h: api.SearchHit) {
    if (!current) return;
    await flush();
    await guard(() => api.addNoteLink(current!.id, h.kind, h.id));
    linkQuery = "";
    linkHits = [];
    current = (await guard(() => api.getNote(current!.id))) ?? current;
  }
  async function unlink(l: api.NoteLink) {
    if (!current) return;
    await guard(() => api.removeNoteLink(current!.id, l.entity_type, l.entity_id));
    current = (await guard(() => api.getNote(current!.id))) ?? current;
  }
  const href = (l: api.NoteLink) =>
    l.entity_type === "event" ? "/calendar" : l.entity_type === "project" ? `/projects/${l.entity_id}` : "/projects";

  onMount(() => {
    (async () => {
      await refresh();
      const id = new URLSearchParams(location.search).get("id");
      if (id) await select(id);
    })();
    return () => void flush();
  });
</script>

<div class="flex h-full">
  <aside class="flex w-72 shrink-0 flex-col border-r border-border">
    <div class="flex gap-2 p-3">
      <button class="btn btn-primary flex-1" onclick={create}>New note</button>
      <button class="btn" onclick={journal}>Today</button>
    </div>
    <ul class="min-h-0 flex-1 overflow-auto">
      {#each notes as n (n.id)}
        <li>
          <button
            class="block w-full px-4 py-2 text-left text-sm hover:bg-surface-2 aria-[current=true]:bg-surface-2"
            aria-current={current?.id === n.id}
            onclick={() => select(n.id)}
          >
            <span class="block truncate font-medium">{n.kind === "journal" ? "📓 " : ""}{n.title}</span>
            <span class="block truncate text-xs text-muted">{new Date(n.updated_at).toLocaleDateString()} · {n.body.slice(0, 40)}</span>
          </button>
        </li>
      {:else}
        <li class="p-4 text-sm text-muted">No notes yet.</li>
      {/each}
    </ul>
  </aside>

  <section class="flex min-w-0 flex-1 flex-col p-6">
    {#if current}
      <div class="mb-3 flex items-center gap-2">
        <input class="input flex-1 font-display text-lg" aria-label="Title" bind:value={title} oninput={edited} />
        <button class="btn" aria-pressed={preview} onclick={() => (preview = !preview)}>{preview ? "Edit" : "Preview"}</button>
        <button class="btn btn-danger" onclick={remove}>Delete</button>
        <span class="w-14 text-xs text-muted" role="status">{saved ? "Saved" : "Saving…"}</span>
      </div>
      {#if preview}
        <article class="prose-note min-h-0 flex-1 overflow-auto rounded-md border border-border p-4 text-sm">
          {@html renderMarkdown(body)}
        </article>
      {:else}
        <textarea class="input min-h-0 flex-1 resize-none font-mono text-sm" aria-label="Body (markdown)" bind:value={body} oninput={edited}></textarea>
      {/if}

      <div class="mt-4 space-y-2 text-sm">
        <div class="flex flex-wrap items-center gap-2">
          <span class="text-muted">Linked to:</span>
          {#each current.links as l (l.entity_type + l.entity_id)}
            <span class="chip">
              <a href={href(l)} class="hover:underline">{l.entity_type}: {l.title ?? "(deleted)"}</a>
              <button aria-label="Remove link" class="ml-1 text-muted hover:text-fg" onclick={() => unlink(l)}>×</button>
            </span>
          {:else}
            <span class="text-muted">nothing</span>
          {/each}
        </div>
        <div class="relative max-w-sm">
          <input class="input" placeholder="Link a task, event or project…" bind:value={linkQuery} oninput={searchLinks} aria-label="Search to link" />
          {#if linkHits.length}
            <ul class="card absolute bottom-full z-10 mb-1 w-full bg-surface">
              {#each linkHits as h (h.kind + h.id)}
                <li><button class="block w-full px-3 py-1.5 text-left hover:bg-surface-2" onclick={() => link(h)}>{h.kind}: {h.title}</button></li>
              {/each}
            </ul>
          {/if}
        </div>
      </div>
    {:else}
      <p class="m-auto text-muted">Select or create a note.</p>
    {/if}
  </section>
</div>

<style>
  .prose-note :global(h1) { font-size: 1.5rem; font-weight: 600; margin: 0.5rem 0; }
  .prose-note :global(h2) { font-size: 1.25rem; font-weight: 600; margin: 0.5rem 0; }
  .prose-note :global(h3) { font-weight: 600; margin: 0.5rem 0; }
  .prose-note :global(p) { margin: 0.5rem 0; }
  .prose-note :global(ul) { list-style: disc; padding-left: 1.5rem; }
  .prose-note :global(ol) { list-style: decimal; padding-left: 1.5rem; }
  .prose-note :global(code), .prose-note :global(pre) { background: var(--color-surface-2); border-radius: 4px; padding: 0 4px; }
  .prose-note :global(pre) { padding: 8px; overflow: auto; }
  .prose-note :global(a) { color: var(--color-primary); text-decoration: underline; }
</style>
