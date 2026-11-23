<script lang="ts">
  import { onMount } from "svelte";
  import { listCalendarRange, localTimezone } from "$lib/api/calendar";
  import type { CalendarItem } from "$lib/api/types";
  import { activeTimers, getStats, type Stats, type TimerView } from "$lib/api/timers";
  import { fmtDuration, fmtHours, fmtTime } from "$lib/format";
  import { toast } from "$lib/stores/toast.svelte";
  import GettingStarted from "$lib/components/GettingStarted.svelte";

  let items = $state<CalendarItem[]>([]);
  let stats = $state<Stats | null>(null);
  let timers = $state<TimerView[]>([]);

  onMount(async () => {
    const s = new Date();
    s.setHours(0, 0, 0, 0);
    const e = new Date(+s + 86_400_000);
    try {
      [items, stats, timers] = await Promise.all([
        listCalendarRange(s.toISOString(), e.toISOString()),
        getStats(localTimezone(), 7),
        activeTimers(),
      ]);
    } catch (err) {
      toast.error(err);
    }
  });
  const today = $derived(stats?.days.at(-1));
</script>

<div class="mx-auto max-w-4xl space-y-6 p-8">
  <header>
    <h1 class="stat-value text-3xl">Today</h1>
    <p class="text-muted">{new Date().toLocaleDateString(undefined, { dateStyle: "full" })}</p>
  </header>
  <GettingStarted />
  <div class="grid gap-4 md:grid-cols-3">
    <div class="card p-4"><p class="text-xs text-muted">Focus today</p><p class="stat-value text-3xl">{fmtHours(today?.focus_ms ?? 0)}</p></div>
    <div class="card p-4"><p class="text-xs text-muted">Completed today</p><p class="stat-value text-3xl">{today?.completed ?? 0}</p></div>
    <div class="card p-4"><p class="text-xs text-muted">Streak</p><p class="stat-value text-3xl">{stats?.current_streak ?? 0} d</p></div>
  </div>
  {#if timers.length}
    <a href="/timers" class="card block p-4 hover:border-primary">
      Timer {timers[0].state}: {fmtDuration(timers[0].remaining_ms ?? timers[0].elapsed_ms)} · {timers[0].label ?? timers[0].kind}
    </a>
  {/if}
  <section aria-labelledby="agenda">
    <h2 id="agenda" class="mb-2 font-display text-lg">Agenda</h2>
    <ul class="space-y-1">
      {#each items as i (i.key)}
        <li class="card flex gap-3 px-3 py-2 text-sm">
          <span class="w-20 text-muted">{i.all_day ? "All day" : fmtTime(i.start_at)}</span>
          <span class={i.completed ? "line-through opacity-60" : ""}>{i.kind === "task" ? "☑ " : ""}{i.title}</span>
        </li>
      {:else}
        <li class="text-muted">Nothing scheduled today.</li>
      {/each}
    </ul>
  </section>
  <p class="text-sm text-muted">Press <kbd>Ctrl</kbd>+<kbd>K</kbd> to search everything.</p>
</div>
