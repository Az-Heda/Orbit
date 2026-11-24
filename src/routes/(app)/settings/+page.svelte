<script lang="ts">
  import { emit } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { applyShortcuts, getAutostart, setAutostart, setWidgetClickThrough, shortcutFailures, toggleWidget } from "$lib/api/desktop";
  import { BLOCK_LABELS, DEFAULT_WIDGET, loadWidget, saveWidget, type WidgetConfig } from "$lib/widget";
  import { listSettings, recentActivity, setSetting } from "$lib/api/settings";
  import type { ActivityEntry, AppError, Setting } from "$lib/api/types";
  import AiSettings from "$lib/components/AiSettings.svelte";
  import CalendarFeeds from "$lib/components/CalendarFeeds.svelte";
  import DataSettings from "$lib/components/DataSettings.svelte";
  import NotificationChannels from "$lib/components/NotificationChannels.svelte";
  import UpdateSettings from "$lib/components/UpdateSettings.svelte";
  import { theme } from "$lib/stores/theme.svelte";
  import { PRESETS, type ThemeMode } from "$lib/theme";

  let settings = $state<Setting[]>([]);
  let activity = $state<ActivityEntry[]>([]);
  let key = $state("");
  let value = $state("");
  let error = $state<string | null>(null);

  let widget = $state<WidgetConfig>(DEFAULT_WIDGET);
  let autostart = $state(false);
  let failures = $state<string[]>([]);
  let remindersOn = $state(true);
  let autoLink = $state(true);
  let lead = $state(10);
  const SHORTCUTS = [
    { key: "shortcuts.palette", label: "Command palette", def: "Ctrl+Alt+Space" },
    { key: "shortcuts.capture", label: "Quick capture", def: "Ctrl+Alt+N" },
    { key: "shortcuts.click_through", label: "Toggle widget click-through", def: "Ctrl+Alt+W" },
  ];
  let combos = $state<Record<string, string>>(Object.fromEntries(SHORTCUTS.map((s) => [s.key, s.def])));

  const report = (e: unknown) => (error = (e as AppError).message ?? String(e));
  async function saveWidgetCfg() {
    try {
      await saveWidget(widget);
      await emit("widget-config-changed", widget);
    } catch (e) {
      report(e);
    }
  }
  async function clickThrough(on: boolean) {
    widget.clickThrough = on;
    try {
      await saveWidget(widget);
      await setWidgetClickThrough(on);
    } catch (e) {
      report(e);
    }
  }
  async function toggleAutostart(on: boolean) {
    try {
      autostart = await setAutostart(on);
    } catch (e) {
      report(e);
      autostart = await getAutostart().catch(() => false);
    }
  }
  async function saveShortcuts() {
    try {
      for (const s of SHORTCUTS) await setSetting(s.key, combos[s.key].trim());
      failures = await applyShortcuts();
    } catch (e) {
      report(e);
    }
  }
  async function saveReminders() {
    try {
      await setSetting("reminders.enabled", remindersOn);
      await setSetting("reminders.lead_minutes", Math.max(0, Math.min(10080, Math.round(lead))));
    } catch (e) {
      report(e);
    }
  }
  async function loadDesktop() {
    widget = await loadWidget();
    autostart = await getAutostart().catch(() => false);
    failures = await shortcutFailures().catch(() => []);
    for (const s of settings) {
      const v = (() => { try { return JSON.parse(s.value); } catch { return undefined; } })();
      if (s.key in combos && typeof v === "string") combos[s.key] = v;
      if (s.key === "reminders.enabled" && typeof v === "boolean") remindersOn = v;
      if (s.key === "timers.auto_link" && typeof v === "boolean") autoLink = v;
      if (s.key === "reminders.lead_minutes" && typeof v === "number") lead = v;
    }
  }

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
    refresh().then(loadDesktop).catch(report);
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

  <section class="card space-y-5 p-6" aria-labelledby="widget">
    <h2 id="widget" class="text-lg">Floating widget</h2>
    <button class="btn" onclick={() => toggleWidget()}>Show / hide widget</button>
    <fieldset class="space-y-2">
      <legend class="text-sm text-muted">Blocks</legend>
      <div class="flex flex-wrap gap-x-5 gap-y-2 text-sm">
        {#each Object.entries(BLOCK_LABELS) as [k, label]}
          <label class="flex items-center gap-2">
            <input type="checkbox" bind:checked={widget.blocks[k as keyof WidgetConfig["blocks"]]} onchange={saveWidgetCfg} />{label}
          </label>
        {/each}
      </div>
    </fieldset>
    <div class="flex flex-wrap items-end gap-5 text-sm">
      <label>
        <span class="block text-muted">Theme</span>
        <select class="input mt-1 w-auto" bind:value={widget.mode} onchange={saveWidgetCfg}>
          <option value="inherit">Same as app</option><option value="light">Light</option><option value="dark">Dark</option>
        </select>
      </label>
      <label class="flex items-center gap-2">
        <input type="checkbox" checked={widget.accent !== null} onchange={(e) => { widget.accent = e.currentTarget.checked ? theme.value.primary : null; saveWidgetCfg(); }} />
        Own accent
      </label>
      {#if widget.accent}
        <input type="color" aria-label="Widget accent" class="h-9 w-16 rounded-md border border-border bg-surface-2 p-0.5" value={widget.accent} oninput={(e) => { widget.accent = e.currentTarget.value; saveWidgetCfg(); }} />
      {/if}
    </div>
    <label class="block max-w-xs space-y-2 text-sm">
      <span class="text-muted">Opacity: {Math.round(widget.opacity * 100)}%</span>
      <input type="range" min="30" max="100" class="w-full accent-[var(--primary)]" value={widget.opacity * 100} oninput={(e) => { widget.opacity = +e.currentTarget.value / 100; saveWidgetCfg(); }} />
    </label>
    <label class="flex items-center gap-2 text-sm">
      <input type="checkbox" checked={widget.clickThrough} onchange={(e) => clickThrough(e.currentTarget.checked)} />
      Click-through (ambient mode)
    </label>
    <p class="text-xs text-muted">In click-through mode the widget ignores the mouse. To get control back, use the tray menu (“Disable widget click-through”) or the click-through shortcut below.</p>
  </section>

  <section class="card space-y-5 p-6" aria-labelledby="desktop">
    <h2 id="desktop" class="text-lg">Desktop</h2>
    <label class="flex items-center gap-2 text-sm">
      <input type="checkbox" checked={autostart} onchange={(e) => toggleAutostart(e.currentTarget.checked)} />
      Launch Orbit at startup (starts hidden in the tray)
    </label>
    <div class="space-y-2 text-sm">
      <p class="text-muted">Global shortcuts (leave empty to disable)</p>
      {#each SHORTCUTS as sc (sc.key)}
        <label class="flex items-center gap-3">
          <span class="w-56">{sc.label}</span>
          <input class="input max-w-48 font-mono" bind:value={combos[sc.key]} placeholder={sc.def} />
        </label>
      {/each}
      <button class="btn" onclick={saveShortcuts}>Save and apply</button>
      {#if failures.length}
        <ul class="text-danger" role="alert">
          {#each failures as f}<li>Could not register {f}</li>{/each}
        </ul>
      {/if}
    </div>
  </section>

  <AiSettings />

  <CalendarFeeds />

  <NotificationChannels />

  <DataSettings />

  <UpdateSettings />

  <section class="card space-y-4 p-6" aria-labelledby="timers">
    <h2 id="timers" class="text-lg">Timers</h2>
    <label class="flex items-center gap-2 text-sm">
      <input type="checkbox" bind:checked={autoLink} onchange={() => setSetting("timers.auto_link", autoLink).catch(report)} />
      Link a focus timer to the task in progress when none is chosen (only if exactly one task is in an active column)
    </label>
  </section>

  <section class="card space-y-4 p-6" aria-labelledby="notif">
    <h2 id="notif" class="text-lg">Reminders</h2>
    <label class="flex items-center gap-2 text-sm"><input type="checkbox" bind:checked={remindersOn} onchange={saveReminders} />Notify before events and task deadlines</label>
    <label class="flex items-center gap-2 text-sm">
      <span class="text-muted">Lead time (minutes)</span>
      <input class="input w-24" type="number" min="0" max="10080" bind:value={lead} onchange={saveReminders} />
    </label>
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
