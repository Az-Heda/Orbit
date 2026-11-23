<script lang="ts">
  import { confirmState } from "$lib/stores/confirm.svelte";

  let dialog = $state<HTMLDialogElement>();
  $effect(() => {
    if (confirmState.pending) dialog?.showModal();
    else dialog?.close();
  });
</script>

<dialog
  bind:this={dialog}
  class="card m-auto w-[min(28rem,92vw)] bg-surface p-6 text-fg backdrop:bg-black/50"
  oncancel={(e) => {
    e.preventDefault();
    confirmState.answer(false);
  }}
  aria-labelledby="confirm-title"
>
  {#if confirmState.pending}
    <h2 id="confirm-title" class="font-display text-lg">Are you sure?</h2>
    <p class="mt-3 text-sm text-muted">{confirmState.pending.message}</p>
    <div class="mt-6 flex justify-end gap-2">
      <button class="btn" onclick={() => confirmState.answer(false)}>Cancel</button>
      <button class="btn btn-primary" onclick={() => confirmState.answer(true)}>
        {confirmState.pending.confirmLabel}
      </button>
    </div>
  {/if}
</dialog>
