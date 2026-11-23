<script lang="ts">
  import type { DayStat } from "$lib/api/timers";
  import { fmtHours } from "$lib/format";

  let { days }: { days: DayStat[] } = $props();
  // Weeks as columns, Monday-first rows. Single hue, light -> dark by magnitude.
  const weeks = $derived.by(() => {
    const cols: (DayStat | null)[][] = [];
    let col: (DayStat | null)[] = [];
    days.forEach((d, i) => {
      const dow = (new Date(d.date + "T00:00:00").getDay() + 6) % 7;
      if (i === 0) col = Array(dow).fill(null);
      if (dow === 0 && col.length) {
        cols.push(col);
        col = [];
      }
      col.push(d);
    });
    if (col.length) cols.push(col);
    return cols;
  });
  const max = $derived(Math.max(1, ...days.map((d) => d.focus_ms)));
  const fill = (d: DayStat) =>
    d.focus_ms === 0
      ? "var(--color-surface-2)"
      : `color-mix(in oklab, var(--color-primary) ${Math.round(25 + (d.focus_ms / max) * 75)}%, var(--color-surface))`;
</script>

<div class="flex gap-[3px] overflow-x-auto pb-1" role="img" aria-label="Focus heatmap, darker means more focus time. See the table view for exact values.">
  {#each weeks as w, i (i)}
    <div class="flex flex-col gap-[3px]">
      {#each w as d, j (j)}
        {#if d}
          <div class="h-3 w-3 rounded-[3px]" style="background:{fill(d)}" title="{d.date}: {fmtHours(d.focus_ms)}, {d.completed} done"></div>
        {:else}
          <div class="h-3 w-3"></div>
        {/if}
      {/each}
    </div>
  {/each}
</div>
<div class="mt-2 flex items-center gap-1 text-xs text-muted" aria-hidden="true">
  Less
  {#each [0, 0.25, 0.5, 0.75, 1] as f}
    <span class="h-3 w-3 rounded-[3px]" style="background:{f === 0 ? 'var(--color-surface-2)' : `color-mix(in oklab, var(--color-primary) ${25 + f * 75}%, var(--color-surface))`}"></span>
  {/each}
  More
</div>
