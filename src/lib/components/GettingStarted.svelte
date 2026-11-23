<script lang="ts">
  import { onMount } from "svelte";
  import { getSetting, setSetting } from "$lib/api/settings";

  const KEY = "onboarding.done";
  let show = $state(false);

  const steps = [
    { href: "/projects", title: "Create a project", text: "Projects hold kanban boards. Add tasks, subtasks and dependencies." },
    { href: "/inbox", title: "Capture anything", text: "Press Ctrl+Alt+N anywhere to drop a thought in the Inbox, triage it later." },
    { href: "/timers", title: "Start a focus timer", text: "Pomodoro or stopwatch; time is tracked even across sleep." },
    { href: "/calendar", title: "Plan your time", text: "Events, recurring series and read-only .ics feed subscriptions." },
    { href: "/assistant", title: "Optional: connect Claude", text: "Add your own API key in Settings. Nothing is shared with the AI unless you allow it." },
    { href: "/settings", title: "Back up your data", text: "Daily automatic backups, plus export and import in Settings." },
  ];

  onMount(async () => {
    try {
      show = (await getSetting(KEY))?.value !== "true";
    } catch {
      show = false;
    }
  });
  async function dismiss() {
    show = false;
    await setSetting(KEY, true).catch(() => {});
  }
</script>

{#if show}
  <section class="card space-y-3 p-5" aria-labelledby="gs">
    <div class="flex items-center justify-between">
      <h2 id="gs" class="font-display text-lg">Getting started with Orbit</h2>
      <button class="btn" onclick={dismiss}>Dismiss</button>
    </div>
    <p class="text-sm text-muted">Everything stays on this computer. Press Ctrl+Alt+Space for the command palette.</p>
    <ul class="grid gap-2 md:grid-cols-2">
      {#each steps as s}
        <li><a href={s.href} class="card block p-3 hover:border-primary"><span class="font-medium">{s.title}</span><span class="block text-sm text-muted">{s.text}</span></a></li>
      {/each}
    </ul>
  </section>
{/if}
