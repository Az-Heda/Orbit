<script lang="ts">
  import { localTimezone } from "$lib/api/calendar";
  import { buildRule, describeRule, parseRule, type Freq } from "$lib/calendar";
  import { fromLocalInput, PRIORITIES, toLocalInput } from "$lib/format";
  import type { TaskTemplate } from "$lib/api/types";
  import type { BoardStore } from "$lib/stores/board.svelte";
  import { confirmState } from "$lib/stores/confirm.svelte";

  let {
    store,
    templates,
    taskId,
    onclose,
    onselect,
  }: {
    store: BoardStore;
    templates: TaskTemplate[];
    taskId: string | null;
    onclose: () => void;
    onselect: (id: string) => void;
  } = $props();

  let dialog = $state<HTMLDialogElement>();
  const task = $derived(store.task(taskId));

  let title = $state("");
  let description = $state("");
  let due = $state("");
  let newItem = $state("");
  let newSub = $state("");
  let blocker = $state("");
  let loadedFor: string | null = null;

  $effect(() => {
    if (taskId && task && loadedFor !== taskId) {
      loadedFor = taskId;
      title = task.title;
      description = task.description ?? "";
      due = toLocalInput(task.due_at);
    }
    if (taskId) {
      if (!dialog?.open) dialog?.showModal();
    } else {
      loadedFor = null;
      if (dialog?.open) dialog.close();
    }
  });

  const candidates = $derived(
    store.allTasks.filter((t) => t.id !== taskId && !store.blockerIds(taskId ?? "").includes(t.id)),
  );

  const repeatParts = $derived(parseRule(task?.recurrence_rule ?? null));
  let interval = $state(1);
  async function setRepeat(freq: Freq) {
    if (!task) return;
    const rule = freq ? buildRule({ freq, interval: Math.max(1, interval), end: "never", count: 1, until: "" }) : null;
    await store.setRecurrence(task.id, rule, localTimezone());
  }

  const saveFields = () =>
    task &&
    store.update(task.id, {
      title,
      description,
      due_at: fromLocalInput(due),
    });

  async function remove() {
    if (!task) return;
    if (!(await confirmState.ask(`Delete "${task.title}" and its subtasks?`, "Delete"))) return;
    await store.remove(task.id);
    onclose();
  }

  async function saveTemplate() {
    if (!task) return;
    const name = prompt("Template name", task.title);
    if (name) await store.saveAsTemplate(task.id, name);
  }

  async function applyTemplate(id: string) {
    if (!task || !id) return;
    await store.instantiateTemplate(id, task.column_id, task.id);
  }
</script>

<dialog
  bind:this={dialog}
  class="card m-auto w-[min(40rem,94vw)] bg-surface p-0 text-fg backdrop:bg-black/50"
  onclose={onclose}
  aria-labelledby="task-title"
>
  {#if task}
    <div class="max-h-[85vh] space-y-5 overflow-y-auto p-6">
      <div class="flex items-start gap-3">
        <label class="sr-only" for="task-title">Title</label>
        <input id="task-title" class="input text-lg font-semibold" bind:value={title} onblur={saveFields} />
        <button class="btn" onclick={() => dialog?.close()} aria-label="Close">✕</button>
      </div>

      <label class="block text-sm">
        <span class="text-muted">Description</span>
        <textarea class="input mt-1 min-h-24" bind:value={description} onblur={saveFields}></textarea>
      </label>

      <div class="grid grid-cols-2 gap-4">
        <label class="block text-sm">
          <span class="text-muted">Priority</span>
          <select
            class="input mt-1"
            value={task.priority}
            onchange={(e) => store.update(task.id, { priority: Number(e.currentTarget.value) })}
          >
            {#each PRIORITIES as p, i}<option value={i}>{p}</option>{/each}
          </select>
        </label>
        <label class="block text-sm">
          <span class="text-muted">Due</span>
          <input type="datetime-local" class="input mt-1" bind:value={due} onchange={saveFields} />
        </label>
      </div>

      <div class="flex flex-wrap items-center gap-2 text-sm">
        <label class="flex items-center gap-2">
          <span class="text-muted">Repeat</span>
          <select
            class="input w-auto"
            disabled={!task.due_at}
            value={repeatParts?.freq ?? (task.recurrence_rule ? "custom" : "")}
            onchange={(e) => setRepeat(e.currentTarget.value as Freq)}
          >
            <option value="">Does not repeat</option>
            <option value="DAILY">Daily</option>
            <option value="WEEKLY">Weekly</option>
            <option value="MONTHLY">Monthly</option>
            <option value="YEARLY">Yearly</option>
            {#if task.recurrence_rule && !repeatParts}<option value="custom" disabled>Custom</option>{/if}
          </select>
        </label>
        {#if task.recurrence_rule}
          <label class="flex items-center gap-1">every <input class="input w-16" type="number" min="1" bind:value={interval} onchange={() => setRepeat((repeatParts?.freq ?? "DAILY") as Freq)} /></label>
          <span class="text-muted">{describeRule(task.recurrence_rule)}</span>
        {:else if !task.due_at}
          <span class="text-muted">Set a due date to repeat.</span>
        {/if}
      </div>

      {#if !task.parent_id}
        <label class="block text-sm">
          <span class="text-muted">Column</span>
          <select
            class="input mt-1"
            value={task.column_id}
            onchange={(e) => store.move(task.id, e.currentTarget.value, 0)}
          >
            {#each store.columns as c (c.id)}<option value={c.id}>{c.name}</option>{/each}
          </select>
        </label>
      {/if}

      <label class="flex items-center gap-2 text-sm">
        <input
          type="checkbox"
          checked={!!task.completed_at}
          onchange={(e) => store.setCompleted(task.id, e.currentTarget.checked)}
        />
        Completed
      </label>

      <section>
        <h3 class="text-sm font-semibold">Checklist</h3>
        <ul class="mt-2 space-y-1">
          {#each store.checklist(task.id) as item (item.id)}
            <li class="flex items-center gap-2 text-sm">
              <input type="checkbox" checked={item.done} onchange={() => store.toggleChecklist(item)} aria-label={item.text} />
              <span class={item.done ? "line-through opacity-60" : ""}>{item.text}</span>
              <button class="ml-auto text-muted hover:text-danger" aria-label="Remove {item.text}" onclick={() => store.removeChecklist(item.id)}>✕</button>
            </li>
          {/each}
        </ul>
        <form
          class="mt-2"
          onsubmit={async (e) => {
            e.preventDefault();
            if (newItem.trim()) await store.addChecklist(task.id, newItem);
            newItem = "";
          }}
        >
          <input class="input" placeholder="+ Add checklist item" bind:value={newItem} />
        </form>
      </section>

      <section>
        <h3 class="text-sm font-semibold">Subtasks</h3>
        <ul class="mt-2 space-y-1">
          {#each store.subtasks(task.id) as s (s.id)}
            <li class="flex items-center gap-2 text-sm">
              <input type="checkbox" checked={!!s.completed_at} onchange={(e) => store.setCompleted(s.id, e.currentTarget.checked)} aria-label="Mark {s.title} done" />
              <button class="text-left hover:text-primary {s.completed_at ? 'line-through opacity-60' : ''}" onclick={() => onselect(s.id)}>{s.title}</button>
            </li>
          {/each}
        </ul>
        <form
          class="mt-2"
          onsubmit={async (e) => {
            e.preventDefault();
            if (newSub.trim()) await store.addSubtask(task.id, newSub);
            newSub = "";
          }}
        >
          <input class="input" placeholder="+ Add subtask" bind:value={newSub} />
        </form>
      </section>

      <section>
        <h3 class="text-sm font-semibold">Blocked by</h3>
        <ul class="mt-2 space-y-1">
          {#each store.blockerIds(task.id) as bid (bid)}
            {@const b = store.task(bid)}
            <li class="flex items-center gap-2 text-sm">
              <span class={b?.completed_at ? "line-through opacity-60" : ""}>{b?.title ?? bid}</span>
              <button class="ml-auto text-muted hover:text-danger" aria-label="Remove blocker" onclick={() => store.removeDependency(task.id, bid)}>✕</button>
            </li>
          {/each}
        </ul>
        <div class="mt-2 flex gap-2">
          <select class="input" bind:value={blocker} aria-label="Add blocker">
            <option value="">Select a task…</option>
            {#each candidates as c (c.id)}<option value={c.id}>{c.title}</option>{/each}
          </select>
          <button
            class="btn"
            disabled={!blocker}
            onclick={async () => {
              await store.addDependency(task.id, blocker);
              blocker = "";
            }}>Add</button
          >
        </div>
      </section>

      <section class="flex flex-wrap items-center gap-2 border-t border-border pt-4">
        <select class="input w-auto" aria-label="Add tasks from template" onchange={(e) => { applyTemplate(e.currentTarget.value); e.currentTarget.value = ""; }}>
          <option value="">Add subtasks from template…</option>
          {#each templates as t (t.id)}<option value={t.id}>{t.name}</option>{/each}
        </select>
        <button class="btn" onclick={saveTemplate}>Save as template</button>
        <button class="btn btn-danger ml-auto" onclick={remove}>Delete task</button>
      </section>
    </div>
  {/if}
</dialog>
