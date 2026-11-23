<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onMount } from "svelte";
  import { updateCheck, updateInstall, updateSkip, type UpdateInfo } from "$lib/api/data";

  let info = $state<UpdateInfo | null>(null);
  let dismissed = $state(false);
  let installing = $state(false);
  let error = $state<string | null>(null);

  const DAY = 24 * 60 * 60 * 1000;
  async function check() {
    try {
      info = await updateCheck(false);
    } catch {
      // Offline or GitHub unreachable: stay quiet, try again later.
    }
  }
  onMount(() => {
    check();
    const t = setInterval(check, DAY);
    return () => clearInterval(t);
  });

  const visible = $derived(!!info?.available && !info.skipped && !dismissed && !!info.release);

  async function install() {
    installing = true;
    error = null;
    try {
      await updateInstall(); // quits this process when it works
    } catch (e) {
      error = (e as { message?: string }).message ?? String(e);
      installing = false;
    }
  }
  async function skip() {
    if (info?.release) await updateSkip(info.release.version).catch(() => {});
    dismissed = true;
  }
</script>

{#if visible && info?.release}
  <div class="fixed bottom-4 right-4 z-50 w-96 max-w-[calc(100vw-2rem)] card space-y-3 p-4 shadow-lg" role="dialog" aria-labelledby="upd-title">
    <h2 id="upd-title" class="text-lg">Orbit {info.release.version} is available</h2>
    <p class="text-sm text-muted">You have {info.current}. Updating closes Orbit and starts the new version; your data is backed up first.</p>
    {#if info.release.notes}
      <pre class="max-h-40 overflow-auto whitespace-pre-wrap text-sm">{info.release.notes}</pre>
    {/if}
    {#if info.install_blocker}
      <p class="text-sm text-muted">Automatic install is unavailable here: {info.install_blocker}.</p>
    {/if}
    {#if error}<p class="text-sm text-danger" role="alert">{error}</p>{/if}
    <div class="flex flex-wrap gap-2">
      {#if info.install_blocker || error}
        <button class="btn btn-primary" onclick={() => openUrl(info!.release!.page_url)}>Open release page</button>
      {:else}
        <button class="btn btn-primary" disabled={installing} onclick={install}>{installing ? "Updating…" : "Update"}</button>
      {/if}
      <button class="btn" disabled={installing} onclick={() => (dismissed = true)}>Later</button>
      <button class="btn" disabled={installing} onclick={skip}>Skip this version</button>
    </div>
  </div>
{/if}
