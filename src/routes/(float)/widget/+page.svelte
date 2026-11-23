<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { listCalendarRange } from "$lib/api/calendar";
  import { captureInbox, inboxCount } from "$lib/api/inbox";
  import { getTask } from "$lib/api/kanban";
  import type { CalendarItem } from "$lib/api/types";
  import { activeTimers, type TimerView } from "$lib/api/timers";
  import { fmtDuration, fmtTime } from "$lib/format";
  import { toast } from "$lib/stores/toast.svelte";
  import { DEFAULT_WIDGET, loadWidget, parseWidget, type WidgetConfig } from "$lib/widget";

  let cfg = $state<WidgetConfig>(DEFAULT_WIDGET);
  let now = $state(new Date());
  let next = $state<CalendarItem | null>(null);
  let timer = $state<TimerView | null>(null);
  let fetchedAt = $state(Date.now());
  let taskTitle = $state<string | null>(null);
  let inbox = $state(0);
  let text = $state("");
  let flash = $state("");

  async function refresh() {
    try {
      const from = new Date();
      const items = await listCalendarRange(from.toISOString(), new Date(+from + 48 * 3_600_000).toISOString());
      next = items.filter((i) => !i.completed && !i.all_day && new Date(i.end_at) > from).sort((a, b) => +new Date(a.start_at) - +new Date(b.start_at))[0] ?? null;
      timer = (await activeTimers())[0] ?? null;
      fetchedAt = Date.now();
      taskTitle = timer?.task_id ? (await getTask(timer.task_id)).title : null;
      inbox = await inboxCount();
    } catch {
      /* the widget stays quiet; the main window reports errors */
    }
  }

  // Time shown is derived from the clock + the server's elapsed value, never counted by the widget.
  const remaining = $derived.by(() => {
    void now;
    if (!timer) return null;
    const extra = timer.state === "running" ? Date.now() - fetchedAt : 0;
    return timer.remaining_ms !== null ? Math.max(0, timer.remaining_ms - extra) : timer.elapsed_ms + extra;
  });

  onMount(() => {
    loadWidget().then((c) => (cfg = c));
    refresh();
    const clock = setInterval(() => (now = new Date()), 1000);
    const poll = setInterval(refresh, 30_000);
    const subs = [
      listen<WidgetConfig>("widget-config-changed", (e) => (cfg = parseWidget(e.payload))),
      listen("domain-event", refresh),
      listen("timer-changed", refresh),
      listen("inbox-changed", refresh),
    ];
    return () => {
      clearInterval(clock);
      clearInterval(poll);
      subs.forEach((u) => void u.then((f) => f()));
    };
  });

  async function capture(e: Event) {
    e.preventDefault();
    if (!text.trim()) return;
    try {
      await captureInbox(text);
      text = "";
      flash = "Captured";
      setTimeout(() => (flash = ""), 1500);
      inbox = await inboxCount();
    } catch (err) {
      toast.error(err);
    }
  }
  const style = $derived(
    `${cfg.accent ? `--primary:${cfg.accent};` : ""}background:color-mix(in srgb, var(--surface) ${Math.round(cfg.opacity * 100)}%, transparent)`,
  );
</script>

<div
  class="flex h-screen flex-col gap-3 rounded-xl border border-border p-4 text-fg"
  data-theme={cfg.mode === "inherit" ? undefined : cfg.mode}
  {style}
>
  <div data-tauri-drag-region class="flex cursor-move items-center justify-between text-xs text-muted">
    <span data-tauri-drag-region class="font-display uppercase tracking-[0.12em]">Orbit</span>
    {#if cfg.clickThrough}<span class="chip" title="Use the tray menu or Ctrl+Alt+W to turn this off">click-through</span>{/if}
  </div>

  {#if cfg.blocks.clock}
    <div>
      <p class="stat-value text-4xl tabular-nums leading-none">{now.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}</p>
      <p class="text-sm text-muted">{now.toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" })}</p>
    </div>
  {/if}

  {#if cfg.blocks.timer}
    <div class="card px-3 py-2 text-sm">
      {#if timer && remaining !== null}
        <p class="text-xs text-muted">{timer.label ?? timer.kind}{timer.state === "paused" ? " · paused" : ""}</p>
        <p class="stat-value text-2xl tabular-nums">{fmtDuration(remaining)}</p>
      {:else}
        <p class="text-muted">No timer running</p>
      {/if}
    </div>
  {/if}

  {#if cfg.blocks.activeTask}
    <div class="text-sm"><span class="text-xs text-muted">Focus task</span><br />{taskTitle ?? "—"}</div>
  {/if}

  {#if cfg.blocks.nextEvent}
    <div class="text-sm">
      <span class="text-xs text-muted">Next up</span><br />
      {#if next}<span class="tabular-nums text-primary">{fmtTime(next.start_at)}</span> {next.title}{:else}Nothing soon{/if}
    </div>
  {/if}

  {#if cfg.blocks.inbox}
    <div class="text-sm"><span class="text-xs text-muted">Inbox</span><br />{inbox} to triage</div>
  {/if}

  {#if cfg.blocks.quickInput}
    <form class="mt-auto" onsubmit={capture}>
      <input class="input" placeholder={flash || "Quick capture…"} aria-label="Quick capture" bind:value={text} />
    </form>
  {/if}
</div>
