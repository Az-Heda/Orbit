<script lang="ts">
  import { onMount } from "svelte";
  import { boardView, listBoards, listProjects } from "$lib/api/kanban";
  import type { Column, Project } from "$lib/api/types";
  import { toast } from "$lib/stores/toast.svelte";

  let { value = $bindable(""), label = "Column" }: { value: string; label?: string } = $props();
  let projects = $state<Project[]>([]);
  let project = $state("");
  let columns = $state<(Column & { board: string })[]>([]);

  async function loadColumns() {
    columns = [];
    if (!project) return;
    try {
      for (const b of await listBoards(project)) {
        const v = await boardView(b.id);
        columns.push(...v.columns.map((c) => ({ ...c, board: b.name })));
      }
      if (!columns.some((c) => c.id === value)) value = columns[0]?.id ?? "";
    } catch (e) {
      toast.error(e);
    }
  }

  onMount(async () => {
    try {
      projects = await listProjects();
      project = projects[0]?.id ?? "";
      await loadColumns();
    } catch (e) {
      toast.error(e);
    }
  });
</script>

<div class="flex flex-wrap items-end gap-2 text-sm">
  <label>
    <span class="text-muted">Project</span>
    <select class="input mt-1 w-auto" bind:value={project} onchange={loadColumns}>
      {#each projects as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
    </select>
  </label>
  <label>
    <span class="text-muted">{label}</span>
    <select class="input mt-1 w-auto" bind:value>
      {#each columns as c (c.id)}<option value={c.id}>{c.name}</option>{/each}
    </select>
  </label>
  {#if !projects.length}<span class="text-muted">Create a project first.</span>{/if}
</div>
