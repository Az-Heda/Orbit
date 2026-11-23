<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { hideWindow } from "$lib/api/desktop";
  import { captureInbox } from "$lib/api/inbox";
  import { toast } from "$lib/stores/toast.svelte";

  let text = $state("");
  let box = $state<HTMLTextAreaElement>();

  onMount(() => {
    box?.focus();
    const un = listen("summoned", () => {
      text = "";
      box?.focus();
    });
    return () => void un.then((f) => f());
  });

  async function submit() {
    if (!text.trim()) return;
    try {
      await captureInbox(text);
      text = "";
      await hideWindow("capture");
    } catch (e) {
      toast.error(e);
    }
  }
  function key(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void submit();
    } else if (e.key === "Escape") void hideWindow("capture");
  }
</script>

<div class="card flex h-screen flex-col gap-2 border-primary bg-surface p-3 shadow-lg">
  <textarea
    bind:this={box}
    bind:value={text}
    onkeydown={key}
    class="input min-h-0 flex-1 resize-none text-base"
    placeholder="Capture anything… (Enter to save, Shift+Enter for a new line, Esc to cancel)"
    aria-label="Quick capture"
  ></textarea>
  <p class="text-xs text-muted">Saved to your Inbox; file it as a task or event later.</p>
</div>
