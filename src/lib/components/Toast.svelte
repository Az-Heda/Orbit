<script lang="ts">
  import { toast } from "$lib/stores/toast.svelte";
</script>

{#if toast.message}
  <div
    role="alert"
    class="fixed right-6 bottom-6 z-50 flex max-w-md items-start gap-3 rounded-lg border border-danger bg-surface px-4 py-3 text-sm shadow-lg"
  >
    <span class="text-danger">{toast.message}</span>
    <button class="text-muted hover:text-fg" aria-label="Dismiss" onclick={() => toast.dismiss()}>✕</button>
  </div>
{/if}
