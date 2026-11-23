<script lang="ts">
  import { onMount } from "svelte";
  import { createProject, deleteProject, listProjects } from "$lib/api/kanban";
  import type { Project } from "$lib/api/types";
  import { confirmState } from "$lib/stores/confirm.svelte";
  import { toast } from "$lib/stores/toast.svelte";

  let projects = $state<Project[]>([]);
  let name = $state("");

  const refresh = async () => (projects = await listProjects());

  async function add(e: Event) {
    e.preventDefault();
    try {
      await createProject(name);
      name = "";
      await refresh();
    } catch (err) {
      toast.error(err);
    }
  }

  async function remove(p: Project) {
    if (!(await confirmState.ask(`Delete project "${p.name}" and all its boards and tasks?`, "Delete"))) return;
    try {
      await deleteProject(p.id);
      await refresh();
    } catch (err) {
      toast.error(err);
    }
  }

  onMount(() => void refresh().catch(toast.error.bind(toast)));
</script>

<div class="mx-auto max-w-3xl space-y-6 p-8">
  <h1 class="font-display text-2xl">Projects</h1>
  <form class="flex gap-2" onsubmit={add}>
    <label class="sr-only" for="pname">Project name</label>
    <input id="pname" class="input" placeholder="New project name" bind:value={name} />
    <button class="btn btn-primary" disabled={!name.trim()}>Create</button>
  </form>

  {#if projects.length === 0}
    <p class="text-muted">No projects yet. Create one to get a board with Backlog, In progress and Done columns.</p>
  {/if}
  <ul class="space-y-2">
    {#each projects as p (p.id)}
      <li class="card flex items-center justify-between p-4">
        <a class="font-medium hover:text-primary" href="/projects/{p.id}">{p.name}</a>
        <button class="btn btn-danger" onclick={() => remove(p)} aria-label="Delete project {p.name}">Delete</button>
      </li>
    {/each}
  </ul>
</div>
