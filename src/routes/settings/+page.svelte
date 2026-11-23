<script lang="ts">
  import { onMount } from "svelte";
  import { listSettings, recentActivity, setSetting } from "$lib/api/settings";
  import type { ActivityEntry, AppError, Setting } from "$lib/api/types";

  let settings = $state<Setting[]>([]);
  let activity = $state<ActivityEntry[]>([]);
  let key = $state("");
  let value = $state("");
  let error = $state<string | null>(null);

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

<main class="mx-auto max-w-2xl space-y-6 p-6">
  <h1 class="text-2xl font-semibold">Orbit</h1>

  <form class="flex gap-2" onsubmit={save}>
    <input class="flex-1 rounded border px-2 py-1" placeholder="key (e.g. ui.theme)" bind:value={key} />
    <input class="flex-1 rounded border px-2 py-1" placeholder="value" bind:value />
    <button class="rounded bg-indigo-600 px-3 py-1 text-white">Save</button>
  </form>
  {#if error}<p class="text-red-600">{error}</p>{/if}

  <section>
    <h2 class="font-medium">Settings</h2>
    <ul>
      {#each settings as s (s.key)}<li><code>{s.key}</code> = <code>{s.value}</code></li>{/each}
    </ul>
  </section>

  <section>
    <h2 class="font-medium">Recent activity</h2>
    <ul class="text-sm">
      {#each activity as a (a.id)}<li>{a.created_at} · {a.actor} {a.action} {a.entity_type}:{a.entity_id}</li>{/each}
    </ul>
  </section>
</main>
