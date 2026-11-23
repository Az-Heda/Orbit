<script lang="ts">
  import { onMount } from "svelte";
  import { localTimezone } from "$lib/api/calendar";
  import * as api from "$lib/api/automation";
  import ColumnPicker from "$lib/components/ColumnPicker.svelte";
  import { confirmState } from "$lib/stores/confirm.svelte";
  import { toast } from "$lib/stores/toast.svelte";

  let rules = $state<api.Rule[]>([]);
  let adding = $state(false);
  let name = $state("");
  let tKind = $state<api.Trigger["kind"]>("task_moved");
  let colKind = $state("done");
  let time = $state("09:00");
  let days = $state<number[]>([]);
  let aKind = $state<api.Action["kind"]>("notify");
  let column = $state("");
  let aTitle = $state("");
  let aBody = $state("");
  let minutes = $state(5);
  let timerKind = $state<"pomodoro" | "countdown">("pomodoro");

  const dayNames = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
  const guard = async (f: () => Promise<unknown>) => {
    try {
      await f();
      rules = await api.listRules();
      return true;
    } catch (e) {
      toast.error(e);
      return false;
    }
  };
  onMount(() => void guard(async () => {}));

  function trigger(): api.Trigger {
    switch (tKind) {
      case "task_moved": return { kind: "task_moved", to_column_kind: colKind || null };
      case "schedule": return { kind: "schedule", time, days, timezone: localTimezone() };
      default: return { kind: tKind } as api.Trigger;
    }
  }
  function action(): api.Action {
    switch (aKind) {
      case "create_task": return { kind: "create_task", column_id: column, title: aTitle, description: aBody || null };
      case "create_note": return { kind: "create_note", title: aTitle, body: aBody };
      case "move_task": return { kind: "move_task", column_id: column };
      case "start_timer": return { kind: "start_timer", timer_kind: timerKind, minutes, label: aTitle || null };
      case "notify": return { kind: "notify", title: aTitle, body: aBody || null };
    }
  }
  async function save() {
    if (await guard(() => api.createRule({ name, trigger: trigger(), action: action() }))) {
      adding = false;
      name = aTitle = aBody = "";
    }
  }
  async function remove(r: api.Rule) {
    if (await confirmState.ask(`Delete rule "${r.name}"?`, "Delete")) void guard(() => api.deleteRule(r.id));
  }
  const needsColumn = $derived(aKind === "create_task" || aKind === "move_task");
  const needsText = $derived(aKind !== "move_task");
</script>

<div class="mx-auto max-w-3xl space-y-6 p-8">
  <header class="flex items-start gap-3">
    <div>
      <h1 class="text-3xl">Automation</h1>
      <p class="mt-2 text-muted">When something happens, Orbit does something. Rules use the same services as you do and are logged as “automation”.</p>
    </div>
    <button class="btn btn-primary ml-auto" onclick={() => (adding = !adding)}>{adding ? "Cancel" : "New rule"}</button>
  </header>

  {#if adding}
    <section class="card space-y-4 p-6 text-sm" aria-label="New rule">
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
      <label class="block">
        <span class="text-muted">Then</span>
        <select class="input mt-1 w-auto" bind:value={aKind}>
          <option value="notify">send a notification</option>
          <option value="create_task">create a task</option>
          <option value="create_note">create a note</option>
          <option value="move_task">move the task</option>
          <option value="start_timer">start a timer</option>
        </select>
      </label>
      {#if needsColumn}<ColumnPicker bind:value={column} />{/if}
      {#if aKind === "start_timer"}
        <div class="flex gap-3">
          <select class="input w-auto" bind:value={timerKind}><option value="pomodoro">pomodoro</option><option value="countdown">countdown</option></select>
          <input class="input w-24" type="number" min="1" max="1440" bind:value={minutes} aria-label="Minutes" />
        </div>
      {/if}
      {#if needsText}
        <label class="block"><span class="text-muted">{aKind === "start_timer" ? "Label (optional)" : "Title"}</span><input class="input mt-1" bind:value={aTitle} /></label>
        {#if aKind !== "start_timer"}<label class="block"><span class="text-muted">Text (optional)</span><textarea class="input mt-1" bind:value={aBody}></textarea></label>{/if}
        <p class="text-xs text-muted">Placeholders: <code>{"{{title}}"}</code> (the item that triggered the rule) and <code>{"{{date}}"}</code>.</p>
      {/if}
      <button class="btn btn-primary" disabled={!name.trim() || (needsColumn && !column)} onclick={save}>Save rule</button>
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
          <p class="text-sm text-muted">When {api.describeTrigger(r.trigger)}, {api.describeAction(r.action)}</p>
        </div>
        <button class="btn btn-danger" onclick={() => remove(r)}>Delete</button>
      </li>
    {:else}
      <li class="text-muted">No rules yet.</li>
    {/each}
  </ul>
  <p class="text-xs text-muted">Safety: a rule never retriggers itself, chains stop after 3 hops, and each rule is limited to 10 runs a minute.</p>
</div>
