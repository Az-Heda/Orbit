<script lang="ts">
  import { applyProposal, getConversation, rejectProposal, undoProposal, type Proposal } from "$lib/api/ai";
  import { toast } from "$lib/stores/toast.svelte";

  let { proposal, onchange }: { proposal: Proposal; onchange: (p: Proposal) => void } = $props();
  let busy = $state(false);

  async function run(action: () => Promise<Proposal>) {
    busy = true;
    try {
      onchange(await action());
    } catch (e) {
      toast.error(e);
      // The proposal may have changed state (e.g. failed); show what the backend now says.
      const fresh = (await getConversation(proposal.conversation_id).catch(() => null))?.proposals.find((p) => p.id === proposal.id);
      if (fresh) onchange(fresh);
    } finally {
      busy = false;
    }
  }
  const apply = () => run(() => applyProposal(proposal.id));
  const reject = () => run(() => rejectProposal(proposal.id));
  const undo = () => run(async () => (await undoProposal(proposal.id))[0]);

  const labels: Record<Proposal["status"], string> = {
    pending: "Awaiting your confirmation",
    applied: "Applied",
    rejected: "Rejected",
    failed: "Failed",
    undone: "Undone",
  };
</script>

<div class="card my-2 space-y-3 p-4 {proposal.preview.destructive ? 'border-danger' : ''}" role="group" aria-label="Proposed change">
  <div class="flex items-start justify-between gap-3">
    <p class="font-medium">{proposal.preview.summary}</p>
    <span class="chip shrink-0" class:text-danger={proposal.status === "failed" || (proposal.preview.destructive && proposal.status === "pending")}>
      {labels[proposal.status]}
    </span>
  </div>
  {#if proposal.preview.destructive && proposal.status === "pending"}
    <p class="text-sm text-danger" role="note">This deletes data. Review it before confirming.</p>
  {/if}
  {#if proposal.preview.changes.length}
    <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
      {#each proposal.preview.changes as c}
        <dt class={c.label === "Warning" ? "text-danger" : "text-muted"}>{c.label}</dt>
        <dd class="min-w-0 break-words {c.label === 'Warning' ? 'text-danger' : ''}">
          {#if c.before !== null}<span class="text-muted line-through">{c.before}</span>{/if}
          {#if c.before !== null && c.after !== null}<span class="text-muted"> → </span>{/if}
          {#if c.after !== null}<span>{c.after}</span>{/if}
        </dd>
      {/each}
    </dl>
  {/if}
  {#if proposal.status === "failed" && proposal.result?.error}
    <p class="text-sm text-danger" role="alert">{proposal.result.error}</p>
  {/if}
  <div class="flex gap-2">
    {#if proposal.status === "pending"}
      <button class="btn {proposal.preview.destructive ? 'btn-danger' : 'btn-primary'}" disabled={busy} onclick={apply}>
        {proposal.preview.destructive ? "Confirm delete" : "Apply"}
      </button>
      <button class="btn" disabled={busy} onclick={reject}>Reject</button>
    {:else if proposal.status === "applied" && proposal.tool_name !== "start_timer"}
      <button class="btn" disabled={busy} onclick={undo}>Undo</button>
    {/if}
  </div>
</div>
