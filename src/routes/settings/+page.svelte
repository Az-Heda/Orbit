<script lang="ts">
  import { onMount } from "svelte";
  import { listSettings, recentActivity, setSetting } from "$lib/api/settings";
  import type { ActivityEntry, AppError, Setting } from "$lib/api/types";
  import { theme } from "$lib/stores/theme.svelte";
  import { PRESETS, type ThemeMode } from "$lib/theme";

  let settings = $state<Setting[]>([]);
  let activity = $state<ActivityEntry[]>([]);
  let key = $state("");
  let value = $state("");
  let error = $state<string | null>(null);

  const modes: { id: ThemeMode; label: string }[] = [
    { id: "system", label: "System" },
    { id: "light", label: "Light" },
    { id: "dark", label: "Dark" },
  ];

  async function refresh() {
    [settings, activity] = await Promise.all([listSettings(), recentActivity(10)]);
  }

  async function save(e: Event) {
    e.preventDefault();
    error = null;
    try {
      let parsed: unknown = value;
      try {
        parsed = JSON.parse(value);
      } catch {
        /* keep as plain string */
      }
      await setSetting(key, parsed);
      key = value = "";
      await refresh();
    } catch (err) {
      error = (err as AppError).message ?? String(err);
    }
  }

  onMount(() => {
    refresh().catch((err) => (error = (err as AppError).message ?? String(err)));
  });
</script>

<div class="mx-auto max-w-3xl space-y-6 p-8">
  <header>
    <h1 class="text-3xl">Settings</h1>
    <p class="mt-2 text-muted">Make Orbit yours. Changes apply instantly and are saved locally.</p>
  </header>

  <section class="card space-y-6 p-6" aria-labelledby="appearance">
    <h2 id="appearance" class="text-lg">Appearance</h2>

    <div class="space-y-2">
      <span class="text-sm text-muted" id="mode-label">Mode</span>
      <div class="seg-group" role="group" aria-labelledby="mode-label">
        {#each modes as m (m.id)}
          <button class="seg" aria-pressed={theme.value.mode === m.id} onclick={() => theme.update({ mode: m.id })}>{m.label}</button>
        {/each}
      </div>
    </div>

    <div class="space-y-2">
      <span class="text-sm text-muted" id="preset-label">Presets</span>
      <div class="flex flex-wrap gap-2" role="group" aria-labelledby="preset-label">
        {#each PRESETS as p (p.name)}
          <button
            class="btn"
            aria-pressed={theme.value.primary === p.primary && theme.value.secondary === p.secondary}
            onclick={() => theme.update({ primary: p.primary, secondary: p.secondary })}
          >
            <span class="h-[18px] w-[18px] rounded-full" style="background:linear-gradient(135deg,{p.primary},{p.secondary})"></span>
            {p.name}
          </button>
        {/each}
      </div>
    </div>

    <div class="flex flex-wrap gap-6">
      <label class="space-y-2 text-sm">
        <span class="block text-muted">Primary</span>
        <input type="color" class="h-9 w-16 rounded-md border border-border bg-surface-2 p-0.5" value={theme.value.primary} oninput={(e) => theme.update({ primary: e.currentTarget.value })} />
      </label>
      <label class="space-y-2 text-sm">
        <span class="block text-muted">Secondary</span>
        <input type="color" class="h-9 w-16 rounded-md border border-border bg-surface-2 p-0.5" value={theme.value.secondary} oninput={(e) => theme.update({ secondary: e.currentTarget.value })} />
      </label>
    </div>

    <label class="block max-w-xs space-y-2 text-sm">
      <span class="text-muted">Glow: {theme.value.glow}%</span>
      <input type="range" min="0" max="100" class="w-full accent-[var(--primary)]" value={theme.value.glow} oninput={(e) => theme.update({ glow: +e.currentTarget.value })} />
    </label>
    <label class="block max-w-xs space-y-2 text-sm">
      <span class="text-muted">Corner radius: {theme.value.radius}px</span>
      <input type="range" min="0" max="24" class="w-full accent-[var(--primary)]" value={theme.value.radius} oninput={(e) => theme.update({ radius: +e.currentTarget.value })} />
    </label>

    <button class="btn" onclick={() => theme.reset()}>Reset to default</button>
  </section>

  <section class="card space-y-4 p-6" aria-labelledby="advanced">
    <h2 id="advanced" class="text-lg">Advanced: raw settings</h2>
    <form class="flex gap-2" onsubmit={save}>
      <input class="input flex-1" placeholder="key (e.g. ui.sidebar)" aria-label="Key" bind:value={key} />
      <input class="input flex-1" placeholder="value" aria-label="Value" bind:value />
      <button class="btn btn-primary" disabled={!key.trim()}>Save</button>
    </form>
    {#if error}<p class="text-danger" role="alert">{error}</p>{/if}
    <ul class="font-mono text-base text-muted">
      {#each settings as s (s.key)}<li><span class="text-fg">{s.key}</span> = {s.value}</li>{/each}
    </ul>
  </section>

  <section class="card space-y-2 p-6" aria-labelledby="activity">
    <h2 id="activity" class="text-lg">Recent activity</h2>
    <ul class="font-mono text-base text-muted">
      {#each activity as a (a.id)}<li>{a.created_at} · {a.actor} {a.action} {a.entity_type}:{a.entity_id.slice(0, 8)}</li>{/each}
    </ul>
  </section>
</div>
