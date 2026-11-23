<script lang="ts">
  let {
    data,
    format,
    label,
  }: { data: { key: string; value: number; tip: string }[]; format: (v: number) => string; label: string } = $props();

  const W = 640, H = 160, PAD = 24;
  const max = $derived(Math.max(1, ...data.map((d) => d.value)));
  const bw = $derived(W / Math.max(1, data.length));
  let hover = $state<number | null>(null);
</script>

<div class="relative">
  <svg viewBox="0 0 {W} {H + PAD}" class="w-full" role="img" aria-label={label}>
    <line x1="0" x2={W} y1={H} y2={H} stroke="var(--color-border)" />
    <line x1="0" x2={W} y1={H * 0.5} y2={H * 0.5} stroke="var(--color-border)" stroke-dasharray="2 4" />
    <text x="0" y="10" font-size="10" fill="var(--color-muted)">{format(max)}</text>
    {#each data as d, i (d.key)}
      {@const h = (d.value / max) * (H - 16)}
      <rect
        x={i * bw + bw * 0.15}
        y={H - h}
        width={bw * 0.7}
        height={Math.max(h, d.value > 0 ? 2 : 0)}
        rx="3"
        fill="var(--color-primary)"
        opacity={hover === null || hover === i ? 1 : 0.5}
      />
      <rect
        x={i * bw}
        y="0"
        width={bw}
        height={H}
        fill="transparent"
        role="presentation"
        onmouseenter={() => (hover = i)}
        onmouseleave={() => (hover = null)}
      />
      {#if i % Math.ceil(data.length / 8) === 0}
        <text x={i * bw + bw / 2} y={H + 16} font-size="10" text-anchor="middle" fill="var(--color-muted)">{d.key.slice(5)}</text>
      {/if}
    {/each}
  </svg>
  {#if hover !== null}
    <div class="pointer-events-none absolute top-0 rounded-md border border-border bg-surface px-2 py-1 text-xs shadow" style="left:clamp(0%, {((hover + 0.5) / data.length) * 100}%, 85%)" role="status">
      {data[hover].tip}
    </div>
  {/if}
</div>
