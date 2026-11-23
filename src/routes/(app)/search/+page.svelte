<script lang="ts">
  import { goto } from "$app/navigation";
  import { onMount } from "svelte";
  import { searchAll, type SearchHit } from "$lib/api/notes";
  import { toast } from "$lib/stores/toast.svelte";

  let query = $state("");
  let hits = $state<SearchHit[]>([]);
  let sel = $state(0);
  let input = $state<HTMLInputElement>();
  let timer: ReturnType<typeof setTimeout> | undefined;
  let seq = 0;

  onMount(() => input?.focus());

  function onInput() {
    clearTimeout(timer);
    timer = setTimeout(async () => {
      const my = ++seq;
      try {
        const r = await searchAll(query, undefined, 30);
        if (my === seq) {
          hits = r;
          sel = 0;
        }
      } catch (e) {
        toast.error(e);
      }
    }, 150);
  }
  const target = (h: SearchHit) =>
    h.kind === "task" || h.kind === "project" ? `/projects/${h.project_id}` : h.kind === "event" ? "/calendar" : h.kind === "note" ? `/notes?id=${h.id}` : "/inbox";
  const open = (h: SearchHit | undefined) => h && goto(target(h));
  function key(e: KeyboardEvent) {
    if (e.key === "ArrowDown") (e.preventDefault(), (sel = Math.min(hits.length - 1, sel + 1)));
    else if (e.key === "ArrowUp") (e.preventDefault(), (sel = Math.max(0, sel - 1)));
    else if (e.key === "Enter") open(hits[sel]);
    document.getElementById(`hit-${sel}`)?.scrollIntoView({ block: "nearest" });
  }
  const parts = (s: string) => s.split(/(⟦[^⟧]*⟧)/).filter(Boolean);
</script>

<div class="mx-auto max-w-2xl space-y-4 p-8">
  <h1 class="font-display text-2xl">Search</h1>
  <input
    bind:this={input}
    class="input text-base"
    placeholder="Search tasks, events, notes, projects, inbox…  (Ctrl+K anywhere)"
    role="combobox"
    aria-expanded={hits.length > 0}
    aria-controls="results"
    aria-activedescendant={hits.length ? `hit-${sel}` : undefined}
    bind:value={query}
    oninput={onInput}
    onkeydown={key}
  />
  <ul id="results" role="listbox" class="space-y-1">
    {#each hits as h, i (h.kind + h.id)}
      <li id="hit-{i}" role="option" aria-selected={i === sel}>
        <button class="card block w-full px-4 py-2 text-left data-[sel=true]:border-primary data-[sel=true]:bg-surface-2" data-sel={i === sel} tabindex="-1" onclick={() => open(h)} onmouseenter={() => (sel = i)}>
          <span class="chip mr-2 text-xs">{h.kind}</span><span class="font-medium">{h.title}</span>
          {#if h.snippet}
            <span class="mt-1 block truncate text-sm text-muted">
              {#each parts(h.snippet) as p}{#if p.startsWith("⟦")}<mark class="rounded bg-primary/20 text-fg">{p.slice(1, -1)}</mark>{:else}{p}{/if}{/each}
            </span>
          {/if}
        </button>
      </li>
    {/each}
  </ul>
  {#if query.trim() && !hits.length}<p class="text-muted" role="status">No results.</p>{/if}
</div>
