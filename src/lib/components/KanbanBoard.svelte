<script lang="ts">
  import { fmtDate, isOverdue, PRIORITIES } from "$lib/format";
  import type { TaskTemplate } from "$lib/api/types";
  import { BoardStore } from "$lib/stores/board.svelte";
  import TaskDialog from "./TaskDialog.svelte";
  import { aiContext } from "$lib/stores/aicontext.svelte";

  let { boardId, templates }: { boardId: string; templates: TaskTemplate[] } = $props();

  const store = new BoardStore();
  $effect(() => {
    void store.load(boardId);
  });

  let openTaskId = $state<string | null>(null);
  $effect(() => {
    if (openTaskId) aiContext.set("taskId", openTaskId, store.task(openTaskId)?.title);
  });
  let dragId = $state<string | null>(null);
  let overColumn = $state<string | null>(null);
  let drafts = $state<Record<string, string>>({});
  let newColumn = $state("");

  const progress = (id: string) => {
    const subs = store.subtasks(id);
    const items = store.checklist(id);
    const total = subs.length + items.length;
    const done = subs.filter((s) => s.completed_at).length + items.filter((i) => i.done).length;
    return total ? `${done}/${total}` : "";
  };

  function dropPosition(columnId: string, clientY: number, container: HTMLElement): number {
    const others = store.rootTasks(columnId).filter((t) => t.id !== dragId);
    const cards = [...container.querySelectorAll<HTMLElement>("[data-card]")].filter((c) => c.dataset.card !== dragId);
    let index = cards.findIndex((c) => {
      const r = c.getBoundingClientRect();
      return clientY < r.top + r.height / 2;
    });
    if (index === -1) index = others.length;
    if (index < others.length) return others[index].position;
    return others.length ? others[others.length - 1].position + 1 : 0;
  }

  async function onDrop(e: DragEvent, columnId: string) {
    e.preventDefault();
    const id = dragId;
    dragId = overColumn = null;
    if (!id) return;
    const pos = dropPosition(columnId, e.clientY, e.currentTarget as HTMLElement);
    await store.move(id, columnId, pos);
  }

  async function quickAdd(e: Event, columnId: string) {
    e.preventDefault();
    const title = drafts[columnId]?.trim();
    if (!title) return;
    drafts[columnId] = "";
    await store.addTask(columnId, title);
  }

  async function addColumn(e: Event) {
    e.preventDefault();
    if (!newColumn.trim()) return;
    await store.addColumn(newColumn, "active");
    newColumn = "";
  }
</script>

<div class="flex h-full min-h-0 flex-1 gap-4 overflow-x-auto p-6">
  {#each store.columns as col (col.id)}
    <section
      class="card flex max-h-full w-72 shrink-0 flex-col {overColumn === col.id ? 'border-primary' : ''}"
      aria-label="{col.name} column"
      ondragover={(e) => {
        if (!dragId) return;
        e.preventDefault();
        overColumn = col.id;
      }}
      ondragleave={() => (overColumn = null)}
      ondrop={(e) => onDrop(e, col.id)}
    >
      <header class="flex items-center justify-between px-3 py-2">
        <h2 class="text-sm font-semibold">
          {col.name}
          <span class="chip ml-1">{store.rootTasks(col.id).length}</span>
        </h2>
        <span class="text-xs text-muted">{col.kind}</span>
      </header>
      <ul class="min-h-8 flex-1 space-y-2 overflow-y-auto px-2 pb-2">
        {#each store.rootTasks(col.id) as t (t.id)}
          {@const blockers = store.openBlockers(t.id)}
          <li data-card={t.id}>
            <div
              class="rounded-md border border-border bg-surface-2 p-2 text-sm {dragId === t.id ? 'opacity-40' : ''}"
              draggable="true"
              role="group"
              ondragstart={(e) => {
                dragId = t.id;
                e.dataTransfer?.setData("text/plain", t.id);
                if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
              }}
              ondragend={() => (dragId = overColumn = null)}
            >
              <div class="flex items-start gap-2">
                <input
                  type="checkbox"
                  class="mt-1"
                  checked={!!t.completed_at}
                  aria-label="Mark {t.title} done"
                  onchange={(e) => store.setCompleted(t.id, e.currentTarget.checked)}
                />
                <button
                  class="flex-1 text-left font-medium hover:text-primary {t.completed_at ? 'line-through opacity-60' : ''}"
                  onclick={() => (openTaskId = t.id)}
                >
                  {t.title}
                </button>
              </div>
              <div class="mt-1 flex flex-wrap gap-1">
                {#if t.priority > 0}
                  <span class="chip {t.priority === 3 ? 'border-danger text-danger' : ''}">{PRIORITIES[t.priority]}</span>
                {/if}
                {#if t.due_at}
                  <span class="chip {isOverdue(t.due_at, !!t.completed_at) ? 'border-danger text-danger' : ''}">
                    Due {fmtDate(t.due_at)}
                  </span>
                {/if}
                {#if t.recurrence_rule}<span class="chip" title="Recurring">↻</span>{/if}
                {#if progress(t.id)}<span class="chip">☑ {progress(t.id)}</span>{/if}
                {#if blockers.length}<span class="chip border-warning text-warning">Blocked</span>{/if}
              </div>
            </div>
          </li>
        {/each}
      </ul>
      <form class="border-t border-border p-2" onsubmit={(e) => quickAdd(e, col.id)}>
        <input
          class="input"
          placeholder="+ Add task"
          aria-label="Add task to {col.name}"
          bind:value={drafts[col.id]}
        />
      </form>
    </section>
  {/each}
  <form class="w-60 shrink-0" onsubmit={addColumn}>
    <input class="input" placeholder="+ Add column" aria-label="New column name" bind:value={newColumn} />
  </form>
</div>

<TaskDialog {store} {templates} taskId={openTaskId} onclose={() => (openTaskId = null)} onselect={(id) => (openTaskId = id)} />
