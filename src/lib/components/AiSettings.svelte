<script lang="ts">
  import { onMount } from "svelte";
  import * as api from "$lib/api/ai";
  import { toast } from "$lib/stores/toast.svelte";
  import { confirmState } from "$lib/stores/confirm.svelte";

  let status = $state<api.KeyStatus | null>(null);
  let keyInput = $state("");
  let config = $state<api.AiConfig>({ model: "claude-opus-5-5", effort: "medium" });
  let sharing = $state<api.Sharing>({ tasks: true, calendar: true, notes: false, inbox: false, memories: true });
  let memories = $state<api.Memory[]>([]);
  let newMemory = $state("");
  let editing = $state<Record<string, string>>({});

  const guard = async <T,>(f: () => Promise<T>) => {
    try {
      return await f();
    } catch (e) {
      toast.error(e);
    }
  };
  onMount(async () => {
    await guard(async () => {
      status = await api.keyStatus();
      config = await api.getConfig();
      sharing = await api.getSharing();
      memories = await api.listMemories();
    });
  });

  async function saveKey(e: Event) {
    e.preventDefault();
    const s = await guard(() => api.setKey(keyInput));
    if (s) {
      status = s;
      keyInput = ""; // the key is never shown again
    }
  }
  async function removeKey() {
    if (!(await confirmState.ask("Remove the stored API key?", "Remove"))) return;
    status = (await guard(api.clearKey)) ?? status;
  }
  async function saveConfig() {
    config = (await guard(() => api.setConfig(config))) ?? config;
  }
  async function saveSharing() {
    sharing = (await guard(() => api.setSharing($state.snapshot(sharing)))) ?? sharing;
  }
  async function addMem(e: Event) {
    e.preventDefault();
    if (await guard(() => api.addMemory(newMemory))) {
      newMemory = "";
      memories = await api.listMemories();
    }
  }
  async function saveMem(m: api.Memory) {
    if (await guard(() => api.updateMemory(m.id, editing[m.id]))) {
      delete editing[m.id];
      memories = await api.listMemories();
    }
  }
  async function removeMem(m: api.Memory) {
    await guard(() => api.deleteMemory(m.id));
    memories = await api.listMemories();
  }

  const SHARE: { key: keyof api.Sharing; label: string; help: string }[] = [
    { key: "tasks", label: "Tasks and projects", help: "Titles, descriptions, due dates, boards" },
    { key: "calendar", label: "Calendar", help: "Events and deadlines" },
    { key: "notes", label: "Notes", help: "Snippets and note text the assistant asks for" },
    { key: "inbox", label: "Inbox", help: "Raw captured text" },
    { key: "memories", label: "Memories", help: "The list below is included in every request" },
  ];
</script>

<section class="card space-y-6 p-6" aria-labelledby="ai">
  <h2 id="ai" class="text-lg">Assistant (Claude)</h2>

  <div class="space-y-2 text-sm">
    <p class="text-muted">
      API key. It is stored in the Windows Credential Manager and only ever used by the Rust backend; it is never shown or sent to the interface.
      Usage is billed to your Anthropic API account, separately from a Claude subscription.
    </p>
    {#if status}
      <p>
        {#if status.configured}Key configured ({status.source === "environment" ? "from ANTHROPIC_API_KEY" : "Credential Manager"}).{:else}No key configured.{/if}
      </p>
    {/if}
    {#if status && !status.can_store}
      <p class="text-muted">Secure storage is only available on Windows. Here, set the ANTHROPIC_API_KEY environment variable before starting Orbit.</p>
    {:else}
      <form class="flex gap-2" onsubmit={saveKey}>
        <input class="input max-w-md font-mono" type="password" autocomplete="off" spellcheck="false" placeholder="sk-ant-…" aria-label="Anthropic API key" bind:value={keyInput} />
        <button class="btn btn-primary" disabled={!keyInput.trim()}>Save key</button>
        {#if status?.configured && status.source === "credential_manager"}<button type="button" class="btn btn-danger" onclick={removeKey}>Remove</button>{/if}
      </form>
    {/if}
  </div>

  <div class="flex flex-wrap items-end gap-4 text-sm">
    <label class="space-y-1">
      <span class="block text-muted">Model</span>
      <input class="input w-64 font-mono" bind:value={config.model} onchange={saveConfig} />
    </label>
    <label class="space-y-1">
      <span class="block text-muted">Effort</span>
      <select class="input w-32" bind:value={config.effort} onchange={saveConfig}>
        {#each ["low", "medium", "high", "xhigh", "max"] as e}<option>{e}</option>{/each}
      </select>
    </label>
  </div>

  <fieldset class="space-y-2 text-sm">
    <legend class="mb-1 text-muted">What the assistant may read (nothing leaves your device unless you chat, and only through these)</legend>
    {#each SHARE as s (s.key)}
      <label class="flex items-start gap-2">
        <input class="mt-1" type="checkbox" bind:checked={sharing[s.key]} onchange={saveSharing} />
        <span>{s.label} <span class="text-muted">· {s.help}</span></span>
      </label>
    {/each}
  </fieldset>

  <div class="space-y-3 text-sm">
    <p class="text-muted">Memories: short facts the assistant keeps. It can only propose new ones, and you confirm. Do not store secrets.</p>
    <ul class="space-y-2">
      {#each memories as m (m.id)}
        <li class="flex items-center gap-2">
          <input class="input flex-1" aria-label="Memory" value={editing[m.id] ?? m.text} oninput={(e) => (editing[m.id] = e.currentTarget.value)} />
          {#if editing[m.id] !== undefined && editing[m.id] !== m.text}<button class="btn" onclick={() => saveMem(m)}>Save</button>{/if}
          <button class="btn btn-danger" aria-label="Delete memory" onclick={() => removeMem(m)}>Delete</button>
        </li>
      {/each}
      {#if memories.length === 0}<li class="text-muted">Nothing remembered yet.</li>{/if}
    </ul>
    <form class="flex gap-2" onsubmit={addMem}>
      <input class="input flex-1" maxlength="300" placeholder="e.g. I prefer deep work in the morning" aria-label="New memory" bind:value={newMemory} />
      <button class="btn" disabled={!newMemory.trim()}>Add</button>
    </form>
  </div>
</section>
