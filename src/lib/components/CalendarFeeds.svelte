<script lang="ts">
  import { onMount } from "svelte";
  import { addExternalCalendar, listExternalCalendars, removeExternalCalendar, syncExternalCalendar } from "$lib/api/calendar";
  import type { ExternalCalendar } from "$lib/api/types";
  import { toast } from "$lib/stores/toast.svelte";
  import { confirmState } from "$lib/stores/confirm.svelte";

  let calendars = $state<ExternalCalendar[]>([]);
  let name = $state("");
  let url = $state("");
  let busy = $state(false);

  const guard = async <T,>(f: () => Promise<T>) => {
    busy = true;
    try {
      return await f();
    } catch (e) {
      toast.error(e);
    } finally {
      busy = false;
    }
  };
  const refresh = async () => (calendars = await listExternalCalendars());

  onMount(() => {
    refresh().catch((e) => toast.error(e));
  });

  async function add(e: Event) {
    e.preventDefault();
    const c = await guard(() => addExternalCalendar(name, url));
    if (c) {
      name = url = "";
      await refresh();
      if (c.last_error) toast.error(c.last_error);
    }
  }
  async function sync(c: ExternalCalendar) {
    await guard(() => syncExternalCalendar(c.id));
    await refresh();
  }
  async function remove(c: ExternalCalendar) {
    if (!(await confirmState.ask(`Remove "${c.name}" and its events from Orbit?`, "Remove"))) return;
    await guard(() => removeExternalCalendar(c.id));
    await refresh();
  }
</script>

<section class="card space-y-4 p-6" aria-labelledby="feeds">
  <h2 id="feeds" class="text-lg">Subscribed calendars (read-only)</h2>
  <p class="text-sm text-muted">
    Paste the private iCal address of a Google or Outlook calendar. Events appear in the calendar, are refreshed hourly, and can
    never be edited here or by the assistant. The address is stored in the Windows Credential Manager and never shown again.
    Nothing is sent anywhere else.
  </p>
  <form class="flex flex-wrap gap-2" onsubmit={add}>
    <input class="input w-40" placeholder="Name" aria-label="Calendar name" bind:value={name} />
    <input class="input min-w-64 flex-1" type="password" autocomplete="off" placeholder="https://…/basic.ics" aria-label="Feed URL" bind:value={url} />
    <button class="btn btn-primary" disabled={busy || !name.trim() || !url.trim()}>Subscribe</button>
  </form>
  <ul class="space-y-2">
    {#each calendars as c (c.id)}
      <li class="flex flex-wrap items-center gap-2 text-sm">
        <span class="font-medium">{c.name}</span>
        <span class="text-muted">{c.event_count} events</span>
        {#if c.last_error}
          <span class="text-danger" role="alert">{c.last_error}</span>
        {:else if c.last_synced_at}
          <span class="text-muted">synced {new Date(c.last_synced_at).toLocaleString()}</span>
        {/if}
        <span class="ml-auto"></span>
        <button class="btn" disabled={busy} onclick={() => sync(c)}>Sync now</button>
        <button class="btn btn-danger" disabled={busy} onclick={() => remove(c)}>Remove</button>
      </li>
    {/each}
  </ul>
</section>
