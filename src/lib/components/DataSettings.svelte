<script lang="ts">
  import { open, save } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import * as api from "$lib/api/data";
  import { setSetting } from "$lib/api/settings";
  import { toast } from "$lib/stores/toast.svelte";
  import { confirmState } from "$lib/stores/confirm.svelte";

  let status = $state<api.BackupStatus | null>(null);
  let backups = $state<api.BackupInfo[]>([]);
  let busy = $state(false);
  let notice = $state<string | null>(null);

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
  async function refresh() {
    [status, backups] = await Promise.all([api.backupStatus(), api.backupList()]);
  }
  onMount(() => {
    refresh().catch((e) => toast.error(e));
  });

  const size = (b: number) => (b > 1048576 ? `${(b / 1048576).toFixed(1)} MB` : `${Math.max(1, Math.round(b / 1024))} KB`);

  async function saveSettings() {
    if (!status) return;
    await guard(async () => {
      await setSetting("backup.enabled", status!.enabled);
      await setSetting("backup.keep", Math.min(60, Math.max(1, Math.round(status!.keep))));
    });
  }
  async function now() {
    if (await guard(api.backupNow)) await refresh();
  }
  async function exportAll() {
    const path = await save({ defaultPath: "orbit-data.db", filters: [{ name: "Orbit data", extensions: ["db"] }] });
    if (!path) return;
    notice = null;
    const ok = await guard(async () => {
      await api.backupExport(path);
      return true;
    });
    if (ok) notice = "Data exported.";
  }
  async function stage(run: () => Promise<void>, what: string) {
    if (!(await confirmState.ask(`${what} replaces all current data. A safety backup is taken first. Continue?`, "Continue"))) return;
    const r = await guard(async () => {
      await run();
      return true;
    });
    if (r) await refresh();
  }
  async function importFile() {
    const path = await open({ multiple: false, filters: [{ name: "Orbit data", extensions: ["db"] }] });
    if (typeof path === "string") await stage(() => api.backupImport(path), "Importing this file");
  }
  const restore = (b: api.BackupInfo) => stage(() => api.backupRestore(b.name), `Restoring ${new Date(b.created_at).toLocaleString()}`);
  async function cancel() {
    await guard(api.backupCancelImport);
    await refresh();
  }
</script>

<section class="card space-y-4 p-6" aria-labelledby="data">
  <h2 id="data" class="text-lg">Data: backups, export and import</h2>
  {#if status}
    {#if status.import_pending}
      <div class="flex flex-wrap items-center gap-2 rounded border border-border p-3 text-sm" role="status">
        <span>An import is ready. Restart Orbit to apply it.</span>
        <span class="ml-auto"></span>
        <button class="btn" disabled={busy} onclick={cancel}>Cancel import</button>
        <button class="btn btn-primary" onclick={api.restartApp}>Restart now</button>
      </div>
    {/if}
    <label class="flex items-center gap-2 text-sm">
      <input type="checkbox" bind:checked={status.enabled} onchange={saveSettings} />
      Back up automatically (at most once a day)
    </label>
    <label class="flex items-center gap-2 text-sm">
      <span class="text-muted">Keep the latest</span>
      <input class="input w-20" type="number" min="1" max="60" bind:value={status.keep} onchange={saveSettings} />
      <span class="text-muted">automatic backups</span>
    </label>
    <p class="text-sm text-muted">Backups are SQLite files in <span class="font-mono">{status.folder}</span>. Export and import use the same format.</p>
    {#if notice}<p class="text-sm" role="status">{notice}</p>{/if}
    <div class="flex flex-wrap gap-2">
      <button class="btn" disabled={busy} onclick={now}>Back up now</button>
      <button class="btn" disabled={busy} onclick={exportAll}>Export all data…</button>
      <button class="btn" disabled={busy} onclick={importFile}>Import data…</button>
    </div>
    <ul class="space-y-1 text-sm">
      {#each backups as b (b.name)}
        <li class="flex flex-wrap items-center gap-2">
          <span>{new Date(b.created_at).toLocaleString()}</span>
          <span class="text-muted">{b.kind}, {size(b.size_bytes)}</span>
          <span class="ml-auto"></span>
          <button class="btn" disabled={busy} onclick={() => restore(b)}>Restore</button>
        </li>
      {:else}
        <li class="text-muted">No backups yet.</li>
      {/each}
    </ul>
  {/if}
</section>
