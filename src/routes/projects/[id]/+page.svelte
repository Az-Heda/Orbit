<script lang="ts">
  import { page } from "$app/state";
  import { createBoard, deleteBoard, listBoards, listProjects, listTemplates, renameBoard } from "$lib/api/kanban";
  import type { Board, Project, TaskTemplate } from "$lib/api/types";
  import KanbanBoard from "$lib/components/KanbanBoard.svelte";
  import { confirmState } from "$lib/stores/confirm.svelte";
  import { toast } from "$lib/stores/toast.svelte";

  let project = $state<Project>();
  let boards = $state<Board[]>([]);
  let templates = $state<TaskTemplate[]>([]);
  let boardId = $state<string>();
  let newBoard = $state("");

  const id = $derived(page.params.id as string);

  async function load() {
    try {
      project = (await listProjects()).find((p) => p.id === id);
      boards = await listBoards(id);
      templates = await listTemplates();
      if (!boards.some((b) => b.id === boardId)) boardId = boards[0]?.id;
    } catch (e) {
      toast.error(e);
    }
  }
  $effect(() => {
    void id;
    void load();
  });

  async function addBoard(e: Event) {
    e.preventDefault();
    try {
      const b = await createBoard(id, newBoard);
      newBoard = "";
      await load();
      boardId = b.id;
    } catch (err) {
      toast.error(err);
    }
  }
  async function rename(b: Board) {
    const name = prompt("Board name", b.name);
    if (!name) return;
    try {
      await renameBoard(b.id, name);
      await load();
    } catch (err) {
      toast.error(err);
    }
  }
  async function remove(b: Board) {
    if (!(await confirmState.ask(`Delete board "${b.name}" and its tasks?`, "Delete"))) return;
    try {
      await deleteBoard(b.id);
      await load();
    } catch (err) {
      toast.error(err);
    }
  }
</script>

<div class="flex h-full flex-col">
  <header class="flex flex-wrap items-center gap-4 border-b border-border px-6 py-4">
    <a href="/projects" class="text-sm text-muted hover:text-fg">← Projects</a>
    <h1 class="font-display text-xl">{project?.name ?? "Project"}</h1>
    <div class="ml-4 flex items-center gap-1" role="tablist" aria-label="Boards">
      {#each boards as b (b.id)}
        <button
          role="tab"
          aria-selected={b.id === boardId}
          class="rounded-md px-3 py-1 text-sm aria-selected:bg-primary aria-selected:text-primary-fg"
          onclick={() => (boardId = b.id)}
          ondblclick={() => rename(b)}
          title="Double-click to rename"
        >
          {b.name}
        </button>
      {/each}
    </div>
    <form class="ml-auto flex gap-2" onsubmit={addBoard}>
      <input class="input w-40" placeholder="New board" aria-label="New board name" bind:value={newBoard} />
      <button class="btn" disabled={!newBoard.trim()}>Add board</button>
    </form>
    {#if boardId && boards.length > 1}
      <button class="btn btn-danger" onclick={() => remove(boards.find((b) => b.id === boardId)!)}>Delete board</button>
    {/if}
  </header>
  {#if boardId}
    {#key boardId}
      <KanbanBoard {boardId} {templates} />
    {/key}
  {/if}
</div>
