<script lang="ts">
  import { onMount } from "svelte";
  import { searchAll, type SearchHit } from "$lib/api/notes";
  import * as api from "$lib/api/timers";
  import { fmtDateTime, fmtDuration, fmtHours } from "$lib/format";
  import { toast } from "$lib/stores/toast.svelte";

  // The backend derives elapsed time from the clock. `tick` only forces a re-render; the displayed
  // value is server elapsed + local time since it was fetched, and is re-synced regularly.
  let active = $state<api.TimerView[]>([]);
  let fetchedAt = $state(Date.now());
  let tick = $state(0);
  let history = $state<api.TimerView[]>([]);
  let sessions = $state<api.FocusSession[]>([]);
  let kind = $state<api.TimerView["kind"]>("pomodoro");
  let minutes = $state(25);
  let label = $state("");
  let taskQuery = $state("");
  let taskHits = $state<SearchHit[]>([]);
  let task = $state<SearchHit | null>(null);
  let announced = "";

  const guard = async <T,>(f: () => Promise<T>) => {
    try {
      return await f();
    } catch (e) {
      toast.error(e);
    }
  };

  async function sync() {
    const a = await guard(() => api.activeTimers());
    if (!a) return;
    const hadRunning = active.some((t) => t.state === "running");
    active = a;
    fetchedAt = Date.now();
    if (hadRunning && !a.length) await loadHistory();
  }
  async function loadHistory() {
    history = (await guard(() => api.timerHistory(10))) ?? history;
    const start = new Date();
    start.setHours(0, 0, 0, 0);
    sessions = (await guard(() => api.listFocusSessions(start.toISOString(), new Date(+start + 86_400_000).toISOString()))) ?? sessions;
  }

  const view = (t: api.TimerView) => {
    void tick;
    const extra = t.state === "running" ? Date.now() - fetchedAt : 0;
    const elapsed = t.elapsed_ms + extra;
    return { elapsed, remaining: t.remaining_ms === null ? null : Math.max(0, t.remaining_ms - extra) };
  };

  onMount(() => {
    sync();
    loadHistory();
    const id = setInterval(() => {
      tick++;
      const done = active.find((t) => t.state === "running" && view(t).remaining === 0);
      if (done && announced !== done.id) {
        announced = done.id;
        sync(); // backend settles it and records the session
      }
    }, 500);
    const resync = setInterval(sync, 15_000);
    const onVisible = () => document.visibilityState === "visible" && sync();
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      clearInterval(id);
      clearInterval(resync);
      document.removeEventListener("visibilitychange", onVisible);
    };
  });

  async function start(phase: "focus" | "break" = "focus", mins = minutes) {
    const r = await guard(() =>
      api.startTimer({
        kind,
        label: label || undefined,
        task_id: task?.id,
        duration_ms: kind === "stopwatch" ? undefined : mins * 60_000,
        phase: kind === "pomodoro" ? phase : undefined,
      }),
    );
    if (r) await sync();
  }
  const act = async (f: () => Promise<unknown>) => {
    if ((await guard(f)) !== undefined) {
      await sync();
      await loadHistory();
    }
  };
  async function findTask() {
    const q = taskQuery.trim();
    taskHits = q ? ((await guard(() => searchAll(q, ["task"], 5))) ?? []) : [];
  }
  const presets = { pomodoro: 25, countdown: 10, stopwatch: 0 } as const;
  const pick = (k: api.TimerView["kind"]) => {
    kind = k;
    minutes = presets[k] || minutes;
  };
  const todayMs = $derived(sessions.reduce((a, s) => a + s.duration_ms, 0));
</script>

<div class="mx-auto max-w-3xl space-y-6 p-8">
  <h1 class="font-display text-2xl">Timers</h1>

  {#each active as t (t.id)}
    {@const v = view(t)}
    <section class="card p-6 text-center" aria-label="Active timer">
      <p class="text-sm text-muted">
        {t.kind}{t.kind === "pomodoro" ? ` · ${t.phase}` : ""}{t.label ? ` · ${t.label}` : ""}{t.state === "paused" ? " · paused" : ""}
      </p>
      <p class="my-2 font-display text-6xl tabular-nums" role="timer" aria-live="off">
        {fmtDuration(v.remaining ?? v.elapsed)}
      </p>
      {#if v.remaining !== null && t.duration_ms}
        <div class="mx-auto h-2 max-w-md overflow-hidden rounded-full bg-surface-2" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round((v.elapsed / t.duration_ms) * 100)} aria-label="Progress">
          <div class="h-full bg-primary" style="width:{Math.min(100, (v.elapsed / t.duration_ms) * 100)}%"></div>
        </div>
      {/if}
      <div class="mt-4 flex justify-center gap-2">
        {#if t.state === "running"}
          <button class="btn" onclick={() => act(() => api.pauseTimer(t.id))}>Pause</button>
        {:else}
          <button class="btn btn-primary" onclick={() => act(() => api.resumeTimer(t.id))}>Resume</button>
        {/if}
        <button class="btn btn-danger" onclick={() => act(() => api.stopTimer(t.id))}>Stop</button>
      </div>
    </section>
  {:else}
    <section class="card space-y-4 p-6" aria-label="Start a timer">
      <div class="flex gap-2" role="group" aria-label="Timer type">
        {#each ["pomodoro", "countdown", "stopwatch"] as k}
          <button class="btn capitalize aria-pressed:bg-primary aria-pressed:text-primary-fg" aria-pressed={kind === k} onclick={() => pick(k as api.TimerView["kind"])}>{k}</button>
        {/each}
      </div>
      <div class="flex flex-wrap items-end gap-3 text-sm">
        {#if kind !== "stopwatch"}
          <label>
            <span class="text-muted">Minutes</span>
            <input class="input mt-1 w-24" type="number" min="1" max="1440" bind:value={minutes} />
          </label>
        {/if}
        <label class="flex-1">
          <span class="text-muted">Label (optional)</span>
          <input class="input mt-1" bind:value={label} maxlength="120" />
        </label>
      </div>
      <div class="relative text-sm">
        {#if task}
          <span class="chip">Task: {task.title} <button aria-label="Unlink task" class="ml-1" onclick={() => (task = null)}>×</button></span>
        {:else}
          <input class="input max-w-sm" placeholder="Link a task (search)…" aria-label="Search a task" bind:value={taskQuery} oninput={findTask} />
          {#if taskHits.length}
            <ul class="card absolute z-10 mt-1 w-full max-w-sm bg-surface">
              {#each taskHits as h (h.id)}
                <li><button class="block w-full px-3 py-1.5 text-left hover:bg-surface-2" onclick={() => { task = h; taskHits = []; taskQuery = ""; }}>{h.title}</button></li>
              {/each}
            </ul>
          {/if}
        {/if}
      </div>
      <div class="flex gap-2">
        <button class="btn btn-primary" onclick={() => start("focus")}>Start {kind === "pomodoro" ? "focus" : ""}</button>
        {#if kind === "pomodoro"}<button class="btn" onclick={() => start("break", 5)}>5 min break</button>{/if}
      </div>
    </section>
  {/each}

  <section aria-labelledby="today">
    <h2 id="today" class="mb-2 font-display text-lg">Today · {fmtHours(todayMs)} focused</h2>
    <ul class="space-y-1 text-sm">
      {#each sessions as s (s.id)}
        <li class="card flex justify-between px-3 py-2">
          <span>{new Date(s.started_at).toLocaleTimeString([], { timeStyle: "short" })}</span>
          <span class="tabular-nums">{fmtHours(s.duration_ms)}{s.task_id ? " · task" : ""}</span>
        </li>
      {:else}
        <li class="text-muted">No focus sessions yet today.</li>
      {/each}
    </ul>
  </section>

  <section aria-labelledby="recent">
    <h2 id="recent" class="mb-2 font-display text-lg">Recent timers</h2>
    <ul class="space-y-1 text-sm">
      {#each history as h (h.id)}
        <li class="flex justify-between px-1 text-muted">
          <span>{h.kind}{h.label ? ` · ${h.label}` : ""}</span>
          <span>{fmtHours(h.elapsed_ms)} · {h.ended_at ? fmtDateTime(h.ended_at) : ""}</span>
        </li>
      {/each}
    </ul>
  </section>
</div>
