<script lang="ts">
  import { onMount } from "svelte";
  import { localTimezone } from "$lib/api/calendar";
  import * as api from "$lib/api/automation";
  import { listChannels, type Channel } from "$lib/api/channels";
  import ColumnPicker from "$lib/components/ColumnPicker.svelte";
  import { confirmState } from "$lib/stores/confirm.svelte";
  import { toast } from "$lib/stores/toast.svelte";

  /** Form state of one step of the chain. */
  interface Step {
    uid: number;
    kind: api.Action["kind"];
    column: string;
    /** Existing column kept as is until the user picks another (the picker starts at the first project). */
    keepColumn: boolean;
    title: string;
    body: string;
    minutes: number;
    timerKind: "pomodoro" | "countdown";
    channelId: string;
  }

  let uid = 0;
  const blankStep = (kind: api.Action["kind"] = "notify"): Step => ({
    uid: uid++,
    kind,
    column: "",
    keepColumn: false,
    title: "",
    body: "",
    minutes: 5,
    timerKind: "pomodoro",
    channelId: "",
  });

  let rules = $state<api.Rule[]>([]);
  let channels = $state<Channel[]>([]);
  let adding = $state(false);
  let editingId = $state<string | null>(null);
  let name = $state("");
  let tKind = $state<api.Trigger["kind"]>("task_moved");
  let colKind = $state("done");
  let keptColumnId = $state<string | null>(null);
  let time = $state("09:00");
  let days = $state<number[]>([]);
  let steps = $state<Step[]>([blankStep()]);

  const dayNames = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
  const channelName = (id: string) => channels.find((c) => c.id === id)?.name;

  const guard = async (f: () => Promise<unknown>) => {
    try {
      await f();
      [rules, channels] = await Promise.all([api.listRules(), listChannels()]);
      return true;
    } catch (e) {
      toast.error(e);
      return false;
    }
  };
  onMount(() => void guard(async () => {}));

  function trigger(): api.Trigger {
    switch (tKind) {
      case "task_moved": return { kind: "task_moved", to_column_kind: colKind || null, to_column_id: keptColumnId };
      case "schedule": return { kind: "schedule", time, days, timezone: localTimezone() };
      default: return { kind: tKind } as api.Trigger;
    }
  }
  function toAction(s: Step): api.Action {
    switch (s.kind) {
      case "create_task": return { kind: "create_task", column_id: s.column, title: s.title, description: s.body || null };
      case "create_note": return { kind: "create_note", title: s.title, body: s.body };
      case "move_task": return { kind: "move_task", column_id: s.column };
      case "start_timer": return { kind: "start_timer", timer_kind: s.timerKind, minutes: s.minutes, label: s.title || null };
      case "notify": return { kind: "notify", title: s.title, body: s.body || null };
      case "send_notification": return { kind: "send_notification", channel_id: s.channelId };
    }
  }
  function toStep(a: api.Action): Step {
    const s = blankStep(a.kind);
    switch (a.kind) {
      case "create_task": return { ...s, column: a.column_id, keepColumn: true, title: a.title, body: a.description ?? "" };
      case "create_note": return { ...s, title: a.title, body: a.body };
      case "move_task": return { ...s, column: a.column_id, keepColumn: true };
      case "start_timer": return { ...s, timerKind: a.timer_kind, minutes: a.minutes, title: a.label ?? "" };
      case "notify": return { ...s, title: a.title, body: a.body ?? "" };
      case "send_notification": return { ...s, channelId: a.channel_id };
    }
  }

  function reset() {
    adding = false;
    editingId = null;
    name = "";
    tKind = "task_moved";
    colKind = "done";
    keptColumnId = null;
    time = "09:00";
    days = [];
    steps = [blankStep()];
  }
  function startNew() {
    if (adding) return reset();
    reset();
    adding = true;
  }
  function startEdit(r: api.Rule) {
    reset();
    adding = true;
    editingId = r.id;
    name = r.name;
    tKind = r.trigger.kind;
    if (r.trigger.kind === "task_moved") {
      colKind = r.trigger.to_column_kind ?? "";
      keptColumnId = r.trigger.to_column_id ?? null;
    }
    if (r.trigger.kind === "schedule") {
      time = r.trigger.time;
      days = [...r.trigger.days];
    }
    steps = r.actions.map(toStep);
  }

  function addStep() {
    if (steps.length < api.MAX_STEPS) steps.push(blankStep());
  }
  function removeStep(i: number) {
    steps.splice(i, 1);
  }
  function moveStep(i: number, d: -1 | 1) {
    const j = i + d;
    if (j < 0 || j >= steps.length) return;
    [steps[i], steps[j]] = [steps[j], steps[i]];
  }

  const needsColumn = (s: Step) => s.kind === "create_task" || s.kind === "move_task";
  const needsText = (s: Step) => s.kind !== "move_task" && s.kind !== "send_notification";
  const stepReady = (s: Step) =>
    (!needsColumn(s) || !!s.column) && (s.kind !== "send_notification" || !!s.channelId) && (s.kind !== "notify" || !!s.title.trim());
  const canSave = $derived(!!name.trim() && steps.length > 0 && steps.every(stepReady));

  async function save() {
    const input = { name, trigger: trigger(), actions: steps.map(toAction) };
    if (await guard(() => (editingId ? api.updateRule(editingId, input) : api.createRule(input)))) reset();
  }
  async function remove(r: api.Rule) {
    if (await confirmState.ask(`Delete rule "${r.name}"?`, "Delete")) void guard(() => api.deleteRule(r.id));
  }
</script>

<div class="mx-auto max-w-3xl space-y-6 p-8">
  <header class="flex items-start gap-3">
    <div>
      <h1 class="text-3xl">Automation</h1>
      <p class="mt-2 text-muted">When something happens, Orbit does one or more things, in order. Rules use the same services as you do and are logged as “automation”.</p>
    </div>
    <button class="btn btn-primary ml-auto" onclick={startNew}>{adding ? "Cancel" : "New rule"}</button>
  </header>

  {#if adding}
    <section class="card space-y-4 p-6 text-sm" aria-label={editingId ? "Edit rule" : "New rule"}>
      <label class="block"><span class="text-muted">Name</span><input class="input mt-1" bind:value={name} /></label>
      <div class="flex flex-wrap gap-4">
        <label>
          <span class="text-muted">When</span>
          <select class="input mt-1 w-auto" bind:value={tKind}>
            <option value="task_created">a task is created</option>
            <option value="task_moved">a task is moved</option>
            <option value="task_completed">a task is completed</option>
            <option value="timer_ended">a timer ends</option>
            <option value="inbox_added">an inbox item is added</option>
            <option value="schedule">on a schedule</option>
          </select>
        </label>
        {#if tKind === "task_moved"}
          <label>
            <span class="text-muted">into a column of kind</span>
            <select class="input mt-1 w-auto" bind:value={colKind}>
              <option value="">any</option><option value="backlog">backlog</option><option value="active">active</option><option value="done">done</option>
            </select>
          </label>
        {/if}
        {#if tKind === "schedule"}
          <label><span class="text-muted">At</span><input class="input mt-1" type="time" bind:value={time} /></label>
          <fieldset>
            <legend class="text-muted">Days (none = every day)</legend>
            <div class="mt-1 flex gap-2">
              {#each dayNames as d, i}
                <label class="flex items-center gap-1"><input type="checkbox" checked={days.includes(i + 1)} onchange={(e) => (days = e.currentTarget.checked ? [...days, i + 1] : days.filter((x) => x !== i + 1))} />{d}</label>
              {/each}
            </div>
          </fieldset>
        {/if}
      </div>

      <ol class="space-y-3" aria-label="Steps">
        {#each steps as s, i (s.uid)}
          <li class="rounded-md border border-border p-4">
            <div class="flex flex-wrap items-center gap-2">
              <span class="text-muted">{i === 0 ? "Then" : "and then"}</span>
              <select class="input w-auto" bind:value={s.kind} aria-label="Step {i + 1} action">
                <option value="notify">show a notification</option>
                <option value="send_notification">send a message to a channel</option>
                <option value="create_task">create a task</option>
                <option value="create_note">create a note</option>
                <option value="move_task">move the task</option>
                <option value="start_timer">start a timer</option>
              </select>
              {#if s.kind === "send_notification"}
                <select class="input w-auto" bind:value={s.channelId} aria-label="Step {i + 1} channel">
                  <option value="" disabled>choose a channel…</option>
                  {#each channels as c (c.id)}<option value={c.id}>{c.name}</option>{/each}
                  {#if s.channelId && !channelName(s.channelId)}<option value={s.channelId}>(deleted channel)</option>{/if}
                </select>
              {/if}
              <span class="ml-auto flex gap-1">
                <button class="btn" disabled={i === 0} onclick={() => moveStep(i, -1)} aria-label="Move step {i + 1} up">↑</button>
                <button class="btn" disabled={i === steps.length - 1} onclick={() => moveStep(i, 1)} aria-label="Move step {i + 1} down">↓</button>
                <button class="btn btn-danger" disabled={steps.length === 1} onclick={() => removeStep(i)} aria-label="Remove step {i + 1}">Remove</button>
              </span>
            </div>

            <div class="mt-3 space-y-3">
              {#if s.kind === "send_notification"}
                {#if channels.length === 0}
                  <p class="text-xs text-muted">No channels yet. Create one in <a class="underline" href="/settings">Settings → Notification channels</a>.</p>
                {:else}
                  <p class="text-xs text-muted">The message comes from the channel's template, filled with this rule's data.</p>
                {/if}
              {/if}
              {#if needsColumn(s)}
                {#if s.keepColumn}
                  <p class="text-xs text-muted">Column unchanged. <button class="underline" onclick={() => (s.keepColumn = false)}>Choose another</button></p>
                {:else}
                  <ColumnPicker bind:value={s.column} />
                {/if}
              {/if}
              {#if s.kind === "start_timer"}
                <div class="flex gap-3">
                  <select class="input w-auto" bind:value={s.timerKind}><option value="pomodoro">pomodoro</option><option value="countdown">countdown</option></select>
                  <input class="input w-24" type="number" min="1" max="1440" bind:value={s.minutes} aria-label="Minutes" />
                </div>
              {/if}
              {#if needsText(s)}
                <label class="block"><span class="text-muted">{s.kind === "start_timer" ? "Label (optional)" : "Title"}</span><input class="input mt-1" bind:value={s.title} /></label>
                {#if s.kind !== "start_timer"}<label class="block"><span class="text-muted">Text (optional)</span><textarea class="input mt-1" bind:value={s.body}></textarea></label>{/if}
              {/if}
            </div>
          </li>
        {/each}
      </ol>
      <p class="text-xs text-muted">
        Steps run top to bottom; if one fails, the rest are skipped and the failure is recorded in the activity log. Placeholders:
        <code>{"{{title}}"}</code> (the item that triggered the rule), <code>{"{{task.title}}"}</code>, <code>{"{{rule.name}}"}</code> and <code>{"{{date}}"}</code>.
      </p>
      <div class="flex gap-3">
        <button class="btn" disabled={steps.length >= api.MAX_STEPS} onclick={addStep}>Add step</button>
        <button class="btn btn-primary" disabled={!canSave} onclick={save}>{editingId ? "Save changes" : "Save rule"}</button>
      </div>
    </section>
  {/if}

  <ul class="space-y-3">
    {#each rules as r (r.id)}
      <li class="card flex items-center gap-4 p-4">
        <label class="flex items-center gap-2">
          <input type="checkbox" checked={r.enabled} onchange={(e) => guard(() => api.setRuleEnabled(r.id, e.currentTarget.checked))} aria-label="Enabled" />
        </label>
        <div class="min-w-0 flex-1">
          <p class="font-medium {r.enabled ? '' : 'opacity-60'}">{r.name}</p>
          <p class="text-sm text-muted">When {api.describeTrigger(r.trigger)}, {api.describeChain(r.actions, channelName)}</p>
        </div>
        <button class="btn" onclick={() => startEdit(r)}>Edit</button>
        <button class="btn btn-danger" onclick={() => remove(r)}>Delete</button>
      </li>
    {:else}
      <li class="text-muted">No rules yet.</li>
    {/each}
  </ul>
  <p class="text-xs text-muted">Safety: a rule never retriggers itself, chains of rules stop after 3 hops, and each rule is limited to 10 runs a minute (a whole chain counts as one run). Channels are limited to 20 messages a minute.</p>
</div>
