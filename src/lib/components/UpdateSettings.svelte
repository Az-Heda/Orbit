<script lang="ts">
  import { onMount } from "svelte";
  import { updateCheck, type UpdateInfo } from "$lib/api/data";
  import { setSetting } from "$lib/api/settings";
  import { toast } from "$lib/stores/toast.svelte";

  let info = $state<UpdateInfo | null>(null);
  let enabled = $state(true);
  let message = $state<string | null>(null);
  let busy = $state(false);

  onMount(async () => {
    try {
      info = await updateCheck(false);
      enabled = info.enabled;
    } catch {
      /* offline */
    }
  });

  async function toggle() {
    try {
      await setSetting("update.check_enabled", enabled);
    } catch (e) {
      toast.error(e);
    }
  }
  async function checkNow() {
    busy = true;
    message = null;
    try {
      info = await updateCheck(true);
      message = info.available ? `Version ${info.release?.version} is available.` : "Orbit is up to date.";
    } catch (e) {
      toast.error(e);
    } finally {
      busy = false;
    }
  }
</script>

<section class="card space-y-4 p-6" aria-labelledby="updates">
  <h2 id="updates" class="text-lg">Updates</h2>
  <label class="flex items-center gap-2 text-sm">
    <input type="checkbox" bind:checked={enabled} onchange={toggle} />
    Check GitHub for new versions (once a day; no personal data is sent)
  </label>
  <div class="flex flex-wrap items-center gap-3 text-sm">
    <button class="btn" disabled={busy} onclick={checkNow}>Check now</button>
    <span class="text-muted">{info ? `Current version ${info.current}` : ""}</span>
    {#if message}<span role="status">{message}</span>{/if}
  </div>
  {#if info?.install_blocker}
    <p class="text-sm text-muted">Automatic install: {info.install_blocker}. You will be offered the release page instead.</p>
  {/if}
</section>
