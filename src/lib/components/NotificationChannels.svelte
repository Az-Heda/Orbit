<script lang="ts">
  import { onMount } from "svelte";
  import * as api from "$lib/api/channels";
  import { toast } from "$lib/stores/toast.svelte";
  import { confirmState } from "$lib/stores/confirm.svelte";

  let channels = $state<api.Channel[]>([]);
  let editing = $state<string | "new" | null>(null);
  let name = $state("");
  let kind = $state<api.ChannelKind>("windows");
  let template = $state(api.DEFAULT_TEMPLATE);
  let chatId = $state("");
  let secret = $state("");
  let busy = $state(false);
  let notice = $state<string | null>(null);

  const current = $derived(channels.find((c) => c.id === editing));
  const remote = $derived(kind !== "windows");
  const secretLabel = $derived(kind === "telegram" ? "Bot token" : "Webhook URL");
  const secretHint = $derived(kind === "telegram" ? "123456:ABC-def… (from @BotFather)" : "https://discord.com/api/webhooks/…");
  // A new channel, or one whose kind changed, must be given a secret.
  const needsSecret = $derived(remote && (editing === "new" || current?.kind !== kind || !current?.has_secret));
  const canSave = $derived(
    !busy && !!name.trim() && !!template.trim() && (kind !== "telegram" || !!chatId.trim()) && (!needsSecret || !!secret.trim()),
  );

  const guard = async <T,>(f: () => Promise<T>) => {
    busy = true;
    notice = null;
    try {
      return await f();
    } catch (e) {
      toast.error(e);
    } finally {
      busy = false;
    }
  };
  const refresh = async () => (channels = await api.listChannels());
  onMount(() => {
    refresh().catch((e) => toast.error(e));
  });

  function startNew() {
    editing = "new";
    name = "";
    kind = "windows";
    template = api.DEFAULT_TEMPLATE;
    chatId = secret = "";
  }
  function startEdit(c: api.Channel) {
    editing = c.id;
    name = c.name;
    kind = c.kind;
    template = c.template;
    chatId = c.chat_id ?? "";
    secret = "";
  }
  function cancel() {
    editing = null;
    secret = "";
  }

  async function save(e: Event) {
    e.preventDefault();
    const input: api.ChannelInput = {
      name,
      kind,
      template,
      chat_id: kind === "telegram" ? chatId : null,
      secret: remote && secret.trim() ? secret : null,
    };
    const ok = await guard(() => (editing === "new" ? api.createChannel(input) : api.updateChannel(editing!, input)));
    if (ok) {
      cancel();
      await refresh();
      notice = "Channel saved.";
    }
  }
  async function test(c: api.Channel) {
    const ok = await guard(async () => {
      await api.sendTestNotification(c.id);
      return true;
    });
    if (ok) notice = `Test message sent to "${c.name}".`;
  }
  async function remove(c: api.Channel) {
    if (!(await confirmState.ask(`Delete channel "${c.name}"? Rules that send to it will fail at that step.`, "Delete"))) return;
    await guard(() => api.deleteChannel(c.id));
    await refresh();
  }
</script>

<section class="card space-y-4 p-6" aria-labelledby="channels">
  <div class="flex items-start gap-3">
    <div>
      <h2 id="channels" class="text-lg">Notification channels</h2>
      <p class="mt-1 text-sm text-muted">
        Named destinations that automation rules can send a message to: a Windows notification, a Telegram chat or a Discord channel.
        Bot tokens and webhook URLs are stored in the Windows Credential Manager, never in the database, and are never shown again
        or sent to the assistant. A failed delivery is logged and never blocks a rule.
      </p>
    </div>
    <button class="btn btn-primary ml-auto shrink-0" onclick={() => (editing ? cancel() : startNew())}>{editing ? "Cancel" : "New channel"}</button>
  </div>

  {#if editing}
    <form class="space-y-3 text-sm" onsubmit={save} aria-label={editing === "new" ? "New channel" : "Edit channel"}>
      <div class="flex flex-wrap gap-3">
        <label class="min-w-48 flex-1"><span class="text-muted">Name</span><input class="input mt-1" bind:value={name} maxlength="80" /></label>
        <label>
          <span class="text-muted">Type</span>
          <select class="input mt-1 w-auto" bind:value={kind}>
            {#each Object.entries(api.KIND_LABELS) as [k, label]}<option value={k}>{label}</option>{/each}
          </select>
        </label>
      </div>
      {#if kind === "telegram"}
        <label class="block"><span class="text-muted">Chat id (a number, or @channelname)</span><input class="input mt-1" bind:value={chatId} placeholder="123456789" /></label>
      {/if}
      {#if remote}
        <label class="block">
          <span class="text-muted">{secretLabel}{needsSecret ? "" : " (leave empty to keep the saved one)"}</span>
          <input class="input mt-1" type="password" autocomplete="off" placeholder={secretHint} bind:value={secret} />
        </label>
      {/if}
      <label class="block">
        <span class="text-muted">Message template</span>
        <textarea class="input mt-1" rows="3" maxlength="2000" bind:value={template}></textarea>
      </label>
      <details class="text-xs text-muted">
        <summary class="cursor-pointer">Placeholders</summary>
        <ul class="mt-1 space-y-0.5">
          {#each api.PLACEHOLDERS as [p, d]}<li><code>{p}</code> {d}</li>{/each}
        </ul>
      </details>
      <button class="btn btn-primary" disabled={!canSave}>Save channel</button>
    </form>
  {/if}

  {#if notice}<p class="text-sm text-muted" role="status">{notice}</p>{/if}

  <ul class="space-y-2">
    {#each channels as c (c.id)}
      <li class="flex flex-wrap items-center gap-2 text-sm">
        <span class="font-medium">{c.name}</span>
        <span class="text-muted">{api.KIND_LABELS[c.kind]}</span>
        {#if c.kind !== "windows" && !c.has_secret}
          <span class="text-danger" role="alert">no {c.kind === "telegram" ? "token" : "webhook"} saved: edit and enter it</span>
        {/if}
        <span class="ml-auto"></span>
        <button class="btn" disabled={busy} onclick={() => test(c)}>Send test</button>
        <button class="btn" disabled={busy} onclick={() => startEdit(c)}>Edit</button>
        <button class="btn btn-danger" disabled={busy} onclick={() => remove(c)}>Delete</button>
      </li>
    {:else}
      <li class="text-sm text-muted">No channels yet.</li>
    {/each}
  </ul>
</section>
