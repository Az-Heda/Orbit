<script lang="ts" module>
  import type { CalendarItem } from "$lib/api/types";
  export type Target = { create: Date } | { item: CalendarItem } | null;
</script>

<script lang="ts">
  import {
    createEvent, deleteEvent, getEvent, localTimezone, overrideOccurrence, restoreOccurrence,
    setTaskOccurrence, skipOccurrence, updateEvent,
  } from "$lib/api/calendar";
  import type { CalendarEvent } from "$lib/api/types";
  import { buildRule, defaultParts, describeRule, parseRule, type Freq, type RuleParts } from "$lib/calendar";
  import { fromLocalInput, toLocalInput } from "$lib/format";
  import { confirmState } from "$lib/stores/confirm.svelte";
  import { toast } from "$lib/stores/toast.svelte";

  let { target, onclose, onchanged }: { target: Target; onclose: () => void; onchanged: () => void } = $props();

  let dialog = $state<HTMLDialogElement>();
  let title = $state("");
  let description = $state("");
  let location = $state("");
  let allDay = $state(false);
  let start = $state("");
  let end = $state("");
  let parts = $state<RuleParts>(defaultParts());
  let customRule = $state<string | null>(null);
  let scope = $state<"series" | "occurrence">("series");
  let series = $state<CalendarEvent | null>(null);
  let loadedKey = "";

  const item = $derived(target && "item" in target ? target.item : null);
  const isTask = $derived(item?.kind === "task");
  const isCreate = $derived(!!target && "create" in target);
  const tz = $derived(series?.timezone ?? localTimezone());
  const dateOnly = (iso: string) => toLocalInput(iso).slice(0, 10);

  async function populate() {
    const key = target ? ("create" in target ? `c${+target.create}` : target.item.key) : "";
    if (key === loadedKey) return;
    loadedKey = key;
    series = null;
    customRule = null;
    parts = defaultParts();
    scope = "series";
    if (!target) return;
    if ("create" in target) {
      const d = new Date(target.create);
      d.setHours(9, 0, 0, 0);
      title = description = location = "";
      allDay = false;
      start = toLocalInput(d.toISOString());
      end = toLocalInput(new Date(+d + 3600_000).toISOString());
      return;
    }
    const it = target.item;
    title = it.title;
    description = it.description ?? "";
    location = it.location ?? "";
    allDay = it.all_day;
    start = toLocalInput(it.start_at);
    end = toLocalInput(it.end_at);
    if (it.kind === "event") {
      try {
        series = await getEvent(it.source_id);
        const p = parseRule(series.recurrence_rule);
        if (p) parts = p;
        else customRule = series.recurrence_rule;
        if (it.recurring) scope = "occurrence";
      } catch (e) {
        toast.error(e);
      }
    }
  }

  $effect(() => {
    void target;
    populate();
    if (target) {
      if (!dialog?.open) dialog?.showModal();
    } else {
      loadedKey = "";
      if (dialog?.open) dialog.close();
    }
  });

  // For all-day events the pickers are date-only; convert to midnight-local ISO for the backend.
  const toIso = (v: string) => fromLocalInput(allDay ? `${v.slice(0, 10)}T00:00` : v);
  const rule = $derived(customRule ?? buildRule(parts));

  async function run(action: () => Promise<unknown>) {
    try {
      await action();
      onchanged();
      onclose();
    } catch (e) {
      toast.error(e);
    }
  }

  function save() {
    if (!target) return;
    if ("create" in target) {
      return run(() =>
        createEvent({
          title, description, location, all_day: allDay,
          start_at: toIso(start), end_at: toIso(end), timezone: localTimezone(),
          recurrence_rule: rule || undefined,
        }),
      );
    }
    const it = target.item;
    if (it.recurring && scope === "occurrence" && it.occurrence_start) {
      return run(() =>
        overrideOccurrence(it.source_id, it.occurrence_start!, {
          title, description, location, start_at: toIso(start), end_at: toIso(end),
        }),
      );
    }
    return run(() =>
      updateEvent(it.source_id, {
        title, description, location, all_day: allDay,
        start_at: toIso(start), end_at: toIso(end), recurrence_rule: rule,
      }),
    );
  }

  async function remove() {
    if (!item) return;
    const msg = item.recurring ? "Delete the entire series?" : `Delete "${item.title}"?`;
    if (await confirmState.ask(msg, "Delete")) run(() => deleteEvent(item.source_id));
  }
</script>

<dialog
  bind:this={dialog}
  class="card m-auto w-[min(36rem,94vw)] bg-surface p-0 text-fg backdrop:bg-black/50"
  onclose={onclose}
  aria-labelledby="ev-title"
>
  {#if target}
    <div class="max-h-[85vh] space-y-4 overflow-y-auto p-6">
      <h2 class="font-display text-lg">{isCreate ? "New event" : isTask ? "Task" : "Event"}</h2>

      {#if item && isTask}
        <p class="text-sm">{item.title}</p>
        <p class="text-sm text-muted">Due {new Date(item.start_at).toLocaleString()}</p>
        <div class="flex flex-wrap gap-2">
          {#if item.board_id}
            <a class="btn" href="/projects/{item.project_id}">Open project</a>
          {/if}
          {#if item.recurring && item.occurrence_start}
            <button
              class="btn"
              onclick={() => run(() => setTaskOccurrence(item.source_id, item.occurrence_start!, item.completed ? "open" : "done"))}
            >
              {item.completed ? "Reopen occurrence" : "Mark occurrence done"}
            </button>
            <button class="btn" onclick={() => run(() => setTaskOccurrence(item.source_id, item.occurrence_start!, "skipped"))}>
              Skip occurrence
            </button>
          {/if}
        </div>
      {:else}
        {#if item?.recurring}
          <fieldset class="flex gap-4 text-sm">
            <legend class="sr-only">Edit scope</legend>
            <label><input type="radio" bind:group={scope} value="occurrence" /> This occurrence</label>
            <label><input type="radio" bind:group={scope} value="series" /> Entire series</label>
          </fieldset>
        {/if}
        <label class="block text-sm">
          <span class="text-muted">Title</span>
          <input id="ev-title" class="input mt-1" bind:value={title} />
        </label>
        <label class="flex items-center gap-2 text-sm">
          <input type="checkbox" bind:checked={allDay} disabled={scope === "occurrence"} /> All day
        </label>
        <div class="grid grid-cols-2 gap-3">
          <label class="block text-sm">
            <span class="text-muted">Start</span>
            <input class="input mt-1" type={allDay ? "date" : "datetime-local"} value={allDay ? start.slice(0, 10) : start} oninput={(e) => (start = e.currentTarget.value)} />
          </label>
          <label class="block text-sm">
            <span class="text-muted">{allDay ? "End (exclusive)" : "End"}</span>
            <input class="input mt-1" type={allDay ? "date" : "datetime-local"} value={allDay ? end.slice(0, 10) : end} oninput={(e) => (end = e.currentTarget.value)} />
          </label>
        </div>
        <label class="block text-sm">
          <span class="text-muted">Location</span>
          <input class="input mt-1" bind:value={location} />
        </label>
        <label class="block text-sm">
          <span class="text-muted">Description</span>
          <textarea class="input mt-1 min-h-20" bind:value={description}></textarea>
        </label>

        {#if scope === "series"}
          <fieldset class="space-y-2 text-sm">
            <legend class="text-muted">Repeat ({tz})</legend>
            {#if customRule}
              <p>Custom rule: <code>{customRule}</code></p>
              <button class="btn" onclick={() => (customRule = null)}>Replace with a simple rule</button>
            {:else}
              <div class="flex flex-wrap items-center gap-2">
                <select class="input w-auto" bind:value={parts.freq} aria-label="Repeat frequency">
                  <option value="">Does not repeat</option>
                  {#each ["DAILY", "WEEKLY", "MONTHLY", "YEARLY"] as f}
                    <option value={f as Freq}>{f[0] + f.slice(1).toLowerCase()}</option>
                  {/each}
                </select>
                {#if parts.freq}
                  every <input class="input w-16" type="number" min="1" bind:value={parts.interval} aria-label="Interval" />
                  <select class="input w-auto" bind:value={parts.end} aria-label="Ends">
                    <option value="never">never ends</option>
                    <option value="count">after N times</option>
                    <option value="until">until date</option>
                  </select>
                  {#if parts.end === "count"}<input class="input w-20" type="number" min="1" bind:value={parts.count} aria-label="Count" />{/if}
                  {#if parts.end === "until"}<input class="input w-40" type="date" bind:value={parts.until} aria-label="Until" />{/if}
                {/if}
              </div>
              {#if rule}<p class="text-muted">{describeRule(rule)}</p>{/if}
            {/if}
          </fieldset>
        {/if}
      {/if}

      <div class="flex flex-wrap items-center gap-2 border-t border-border pt-4">
        {#if item && !isTask && item.recurring && item.occurrence_start}
          <button class="btn" onclick={() => run(() => skipOccurrence(item.source_id, item.occurrence_start!))}>Skip this occurrence</button>
          {#if item.overridden}
            <button class="btn" onclick={() => run(() => restoreOccurrence(item.source_id, item.occurrence_start!))}>Restore to series</button>
          {/if}
        {/if}
        {#if item && !isTask}<button class="btn btn-danger" onclick={remove}>Delete</button>{/if}
        <span class="ml-auto"></span>
        <button class="btn" onclick={() => dialog?.close()}>{isTask ? "Close" : "Cancel"}</button>
        {#if !isTask}<button class="btn btn-primary" onclick={save} disabled={!title.trim()}>Save</button>{/if}
      </div>
    </div>
  {/if}
</dialog>
