<script lang="ts">
  import { onMount } from "svelte";
  import { getStats, type Stats } from "$lib/api/timers";
  import { localTimezone } from "$lib/api/calendar";
  import BarChart from "$lib/components/BarChart.svelte";
  import Heatmap from "$lib/components/Heatmap.svelte";
  import { fmtHours } from "$lib/format";
  import { toast } from "$lib/stores/toast.svelte";

  let range = $state(30);
  let stats = $state<Stats | null>(null);
  let heat = $state<Stats | null>(null);
  let table = $state(false);

  async function load() {
    try {
      [stats, heat] = await Promise.all([getStats(localTimezone(), range), getStats(localTimezone(), 182)]);
    } catch (e) {
      toast.error(e);
    }
  }
  onMount(load);
  const maxProject = $derived(Math.max(1, ...(stats?.by_project.map((p) => p.focus_ms) ?? [1])));
</script>

<div class="mx-auto max-w-4xl space-y-6 p-8">
  <header class="flex items-center gap-3">
    <h1 class="stat-value text-3xl">Stats</h1>
    <select class="input ml-auto w-auto" aria-label="Range" bind:value={range} onchange={load}>
      <option value={7}>Last 7 days</option>
      <option value={30}>Last 30 days</option>
      <option value={90}>Last 90 days</option>
    </select>
    <button class="btn" aria-pressed={table} onclick={() => (table = !table)}>{table ? "Charts" : "Table view"}</button>
  </header>

  {#if stats && heat}
    <div class="grid grid-cols-2 gap-3 md:grid-cols-4">
      {#each [["Focus time", fmtHours(stats.total_focus_ms)], ["Tasks completed", String(stats.total_completed)], ["Current streak", `${stats.current_streak} d`], ["Longest streak", `${stats.longest_streak} d`]] as [k, v]}
        <div class="card p-4"><p class="text-xs text-muted">{k}</p><p class="stat-value text-3xl tabular-nums">{v}</p></div>
      {/each}
    </div>

    {#if table}
      <table class="w-full text-sm">
        <caption class="sr-only">Daily focus and completions</caption>
        <thead><tr class="text-left text-muted"><th class="py-1">Date</th><th>Focus</th><th>Sessions</th><th>Completed</th></tr></thead>
        <tbody>
          {#each [...stats.days].reverse() as d (d.date)}
            <tr class="border-t border-border"><td class="py-1">{d.date}</td><td>{fmtHours(d.focus_ms)}</td><td>{d.sessions}</td><td>{d.completed}</td></tr>
          {/each}
        </tbody>
      </table>
    {:else}
      <section class="card p-4">
        <h2 class="mb-2 font-medium">Focus per day</h2>
        <BarChart label="Focus time per day" format={fmtHours} data={stats.days.map((d) => ({ key: d.date, value: d.focus_ms, tip: `${d.date}: ${fmtHours(d.focus_ms)}` }))} />
      </section>
      <section class="card p-4">
        <h2 class="mb-2 font-medium">Tasks completed per day</h2>
        <BarChart label="Tasks completed per day" format={(v) => String(v)} data={stats.days.map((d) => ({ key: d.date, value: d.completed, tip: `${d.date}: ${d.completed} completed` }))} />
      </section>
      <section class="card p-4">
        <h2 class="mb-3 font-medium">Focus by project</h2>
        <ul class="space-y-2 text-sm">
          {#each stats.by_project as p (p.project_id ?? "none")}
            <li class="flex items-center gap-3">
              <span class="w-40 truncate">{p.name}</span>
              <span class="h-2 flex-1 overflow-hidden rounded-full bg-surface-2"><span class="block h-full rounded-full bg-primary" style="width:{(p.focus_ms / maxProject) * 100}%"></span></span>
              <span class="w-16 text-right tabular-nums text-muted">{fmtHours(p.focus_ms)}</span>
            </li>
          {:else}
            <li class="text-muted">No focus sessions in this range.</li>
          {/each}
        </ul>
      </section>
      <section class="card p-4">
        <h2 class="mb-3 font-medium">Last 26 weeks</h2>
        <Heatmap days={heat.days} />
      </section>
    {/if}
  {/if}
</div>
