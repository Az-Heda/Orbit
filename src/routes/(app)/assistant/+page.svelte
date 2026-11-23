<script lang="ts">
  import { onMount, tick } from "svelte";
  import { goto } from "$app/navigation";
  import { keyStatus } from "$lib/api/ai";
  import ProposalCard from "$lib/components/ProposalCard.svelte";
  import { aiContext } from "$lib/stores/aicontext.svelte";
  import { AssistantStore, blocksOf, textOf } from "$lib/stores/assistant.svelte";
  import { renderMarkdown } from "$lib/markdown";

  const store = new AssistantStore();
  let input = $state("");
  let log = $state<HTMLElement>();
  let hasKey = $state(true);

  const chips = $derived(
    (
      [
        ["projectId", "Project"],
        ["boardId", "Board"],
        ["taskId", "Task"],
        ["eventId", "Event"],
        ["noteId", "Note"],
      ] as const
    )
      .filter(([f]) => aiContext[f])
      .map(([f, label]) => ({ f, text: `${label}: ${aiContext.labels[f] ?? aiContext[f]!.slice(0, 8)}` })),
  );

  onMount(() => {
    void store.start();
    keyStatus().then((s) => (hasKey = s.configured)).catch(() => {});
    return () => store.stop();
  });

  $effect(() => {
    void store.messages.length;
    void store.streaming;
    void store.proposals.length;
    void tick().then(() => log?.scrollTo({ top: log.scrollHeight }));
  });

  async function submit(e: Event) {
    e.preventDefault();
    const t = input;
    input = "";
    await store.send(t);
  }
  function onKey(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing) void submit(e);
  }
</script>

<div class="flex h-full">
  <aside class="flex w-64 shrink-0 flex-col gap-2 border-r border-border bg-surface p-3" aria-label="Conversations">
    <button class="btn btn-primary justify-center" onclick={() => store.open(null)}>New chat</button>
    <ul class="flex-1 space-y-1 overflow-auto">
      {#each store.conversations as c (c.id)}
        <li class="group flex items-center">
          <button class="min-w-0 flex-1 truncate rounded-md px-2 py-1.5 text-left text-sm hover:bg-primary/10" aria-current={c.id === store.currentId ? "true" : undefined} onclick={() => store.open(c.id)}>
            <span class={c.id === store.currentId ? "text-primary" : ""}>{c.title}</span>
          </button>
          <button class="px-2 text-muted opacity-0 hover:text-danger focus:opacity-100 group-hover:opacity-100" aria-label="Delete conversation {c.title}" onclick={() => store.remove(c.id)}>×</button>
        </li>
      {/each}
    </ul>
  </aside>

  <section class="flex min-w-0 flex-1 flex-col">
    <header class="border-b border-border px-8 py-4">
      <h1 class="font-display text-2xl uppercase tracking-[0.1em]">Assistant</h1>
    </header>

    <div bind:this={log} class="flex-1 space-y-4 overflow-auto px-8 py-6" aria-live="polite">
      {#if !hasKey}
        <div class="card p-4 text-sm" role="note">
          No API key yet. <button class="text-primary underline" onclick={() => goto("/settings")}>Add one in Settings</button> to start chatting.
        </div>
      {/if}
      {#if store.messages.length === 0 && !store.pendingUser}
        <p class="text-muted">Ask about your schedule or tasks, or tell me what to change. I propose changes and you confirm each one.</p>
      {/if}

      {#each store.messages as m (m.id)}
        {@const text = textOf(m)}
        {#if m.role === "user" && text}
          <div class="ml-auto max-w-[75%] whitespace-pre-wrap rounded-lg bg-primary/15 px-4 py-2 text-sm">{text}</div>
        {:else if m.role === "assistant"}
          {@const blocks = blocksOf(m)}
          <div class="max-w-[85%] space-y-1">
            {#if text}<div class="prose-chat text-sm">{@html renderMarkdown(text)}</div>{/if}
            {#each store.proposalsFor(blocks) as p (p.id)}
              <ProposalCard proposal={p} onchange={(n) => store.replaceProposal(n)} />
            {/each}
          </div>
        {/if}
      {/each}

      {#if store.pendingUser}
        <div class="ml-auto max-w-[75%] whitespace-pre-wrap rounded-lg bg-primary/15 px-4 py-2 text-sm">{store.pendingUser}</div>
      {/if}
      {#if store.busy}
        <div class="max-w-[85%] space-y-1">
          {#if store.streaming}<div class="prose-chat text-sm">{@html renderMarkdown(store.streaming)}</div>{/if}
          {#each store.proposals.filter((p) => p.status === "pending" && !store.messages.some((m) => m.role === "assistant" && blocksOf(m).some((b) => b.type === "tool_use" && b.id === p.preview.tool_use_id))) as p (p.id)}
            <ProposalCard proposal={p} onchange={(n) => store.replaceProposal(n)} />
          {/each}
          <p class="text-sm text-muted">
            {store.tools.length ? `Working: ${store.tools[store.tools.length - 1].replaceAll("_", " ")}…` : store.streaming ? "" : "Thinking…"}
          </p>
        </div>
      {/if}
      {#if store.error}
        <p class="card border-danger p-3 text-sm text-danger" role="alert">{store.error}</p>
      {/if}
    </div>

    <form class="space-y-2 border-t border-border px-8 py-4" onsubmit={submit}>
      {#if chips.length || aiContext.view}
        <div class="flex flex-wrap items-center gap-2" aria-label="Context shared with this message">
          <span class="text-xs text-muted">Context:</span>
          {#if aiContext.view}<span class="chip">{aiContext.view}</span>{/if}
          {#each chips as c (c.f)}
            <span class="chip gap-1">{c.text}<button type="button" class="hover:text-danger" aria-label="Remove {c.text} from context" onclick={() => aiContext.clear(c.f)}>×</button></span>
          {/each}
        </div>
      {/if}
      <div class="flex gap-2">
        <textarea class="input min-h-[2.75rem] flex-1 resize-none" rows="2" placeholder="Ask or instruct… (Enter to send, Shift+Enter for a new line)" aria-label="Message" bind:value={input} onkeydown={onKey} disabled={store.busy}></textarea>
        {#if store.busy}
          <button type="button" class="btn" onclick={() => store.cancel()}>Stop</button>
        {:else}
          <button class="btn btn-primary" disabled={!input.trim()}>Send</button>
        {/if}
      </div>
    </form>
  </section>
</div>
