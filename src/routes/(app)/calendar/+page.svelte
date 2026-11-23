<script lang="ts">
  import { open, save } from "@tauri-apps/plugin-dialog";
  import { exportIcsFile, importIcsFile, listCalendarRange, localTimezone } from "$lib/api/calendar";
  import type { CalendarItem } from "$lib/api/types";
  import { addDays, days, overlapsDay, sameDay, shift, startOfDay, viewRange, type View } from "$lib/calendar";
  import { aiContext } from "$lib/stores/aicontext.svelte";
  import EventDialog, { type Target } from "$lib/components/EventDialog.svelte";
  import { fmtTime } from "$lib/format";
  import { toast } from "$lib/stores/toast.svelte";

  let view = $state<View>("month");
  let anchor = $state(startOfDay(new Date()));
  let items = $state<CalendarItem[]>([]);
  let target = $state<Target>(null);
  $effect(() => {
    if (target && "item" in target && target.item.kind === "event") aiContext.set("eventId", target.item.source_id, target.item.title);
  });
  let notice = $state("");

  const range = $derived(viewRange(view, anchor));
  const gridDays = $derived(days(range[0], range[1]));
  const title = $derived(
    view === "day"
      ? anchor.toLocaleDateString(undefined, { dateStyle: "full" })
      : view === "week"
        ? `${range[0].toLocaleDateString(undefined, { dateStyle: "medium" })} – ${addDays(range[1], -1).toLocaleDateString(undefined, { dateStyle: "medium" })}`
        : anchor.toLocaleDateString(undefined, { month: "long", year: "numeric" }),
  );

  let seq = 0;
  async function load() {
    const my = ++seq;
    try {
      const r = await listCalendarRange(range[0].toISOString(), range[1].toISOString());
      if (my === seq) items = r;
    } catch (e) {
      toast.error(e);
    }
  }
  $effect(() => {
    void range;
    void load();
  });

  const onDay = (d: Date) => items.filter((i) => overlapsDay(i, d));

  async function exportIcs() {
    const path = await save({ defaultPath: "orbit-calendar.ics", filters: [{ name: "iCalendar", extensions: ["ics"] }] });
    if (!path) return;
    try {
      await exportIcsFile(path);
      notice = "Calendar exported.";
    } catch (e) {
      toast.error(e);
    }
  }
  async function importIcs() {
    const path = await open({ multiple: false, filters: [{ name: "iCalendar", extensions: ["ics"] }] });
    if (typeof path !== "string") return;
    try {
      const s = await importIcsFile(path, localTimezone());
      notice = `Imported ${s.created} event(s), skipped ${s.skipped_duplicates} duplicate(s)` + (s.warnings.length ? `, ${s.warnings.length} warning(s): ${s.warnings.slice(0, 3).join("; ")}` : "");
      await load();
    } catch (e) {
      toast.error(e);
    }
  }

  const chipClass = (i: CalendarItem) =>
    i.kind === "task"
      ? `border-warning text-warning ${i.completed ? "line-through opacity-60" : ""}`
      : "border-primary bg-primary/10 text-fg";
</script>

{#snippet chip(i: CalendarItem)}
  <button
    class="block w-full truncate rounded border px-1.5 py-0.5 text-left text-xs {chipClass(i)}"
    onclick={() => (target = { item: i })}
    title={i.title}
  >
    {#if i.kind === "task"}☑ {:else if i.recurring}↻ {/if}{#if !i.all_day && i.kind === "event"}{fmtTime(i.start_at)} {/if}{i.title}
  </button>
{/snippet}

<div class="flex h-full flex-col">
  <header class="flex flex-wrap items-center gap-3 border-b border-border px-6 py-3">
    <h1 class="font-display text-xl">Calendar</h1>
    <div class="flex items-center gap-1">
      <button class="btn" aria-label="Previous" onclick={() => (anchor = shift(view, anchor, -1))}>‹</button>
      <button class="btn" onclick={() => (anchor = startOfDay(new Date()))}>Today</button>
      <button class="btn" aria-label="Next" onclick={() => (anchor = shift(view, anchor, 1))}>›</button>
    </div>
    <span class="font-medium" aria-live="polite">{title}</span>
    <div class="ml-auto flex items-center gap-2">
      <div class="flex rounded-md border border-border" role="group" aria-label="View">
        {#each ["day", "week", "month"] as v}
          <button class="px-3 py-1 text-sm capitalize aria-pressed:bg-primary aria-pressed:text-primary-fg" aria-pressed={view === v} onclick={() => (view = v as View)}>{v}</button>
        {/each}
      </div>
      <button class="btn" onclick={importIcs}>Import .ics</button>
      <button class="btn" onclick={exportIcs}>Export .ics</button>
      <button class="btn btn-primary" onclick={() => (target = { create: view === "day" ? anchor : startOfDay(new Date()) })}>New event</button>
    </div>
  </header>
  {#if notice}<p class="border-b border-border px-6 py-2 text-sm text-success" role="status">{notice}</p>{/if}

  <div class="min-h-0 flex-1 overflow-auto p-4">
    {#if view === "month"}
      <div class="grid grid-cols-7 gap-px rounded-lg border border-border bg-border text-sm" role="grid">
        {#each ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] as d}
          <div class="bg-surface-2 px-2 py-1 text-center text-xs font-semibold text-muted" role="columnheader">{d}</div>
        {/each}
        {#each gridDays as d (+d)}
          {@const list = onDay(d)}
          <div
            class="min-h-28 bg-surface p-1 {d.getMonth() !== anchor.getMonth() ? 'opacity-50' : ''}"
            role="gridcell"
            tabindex="-1"
            ondblclick={() => (target = { create: d })}
          >
            <div class="mb-1 flex justify-between px-1 text-xs {sameDay(d, new Date()) ? 'font-bold text-primary' : 'text-muted'}">
              <span>{d.getDate()}</span>
            </div>
            <div class="space-y-0.5">
              {#each list.slice(0, 4) as i (i.key)}{@render chip(i)}{/each}
              {#if list.length > 4}
                <button class="px-1 text-xs text-muted hover:text-fg" onclick={() => { anchor = d; view = "day"; }}>+{list.length - 4} more</button>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    {:else}
      <div class="grid gap-3" style="grid-template-columns: repeat({gridDays.length}, minmax(0, 1fr))">
        {#each gridDays as d (+d)}
          <section class="card min-h-64 p-2" aria-label={d.toDateString()}>
            <h2 class="mb-2 text-xs font-semibold {sameDay(d, new Date()) ? 'text-primary' : 'text-muted'}">
              {d.toLocaleDateString(undefined, { weekday: "short", day: "numeric", month: "short" })}
            </h2>
            <div class="space-y-1">
              {#each onDay(d) as i (i.key)}{@render chip(i)}{/each}
            </div>
            <button class="mt-2 text-xs text-muted hover:text-fg" onclick={() => (target = { create: d })}>+ Add</button>
          </section>
        {/each}
      </div>
    {/if}
  </div>
</div>

<EventDialog {target} onclose={() => (target = null)} onchanged={load} />
