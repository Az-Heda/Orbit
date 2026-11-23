<script lang="ts">
  import Toast from "$lib/components/Toast.svelte";
  let { children } = $props();
</script>

<svelte:head>
  <style>
    html,
    body {
      background: transparent !important;
      overflow: hidden;
    }
  </style>
</svelte:head>

{@render children()}
<Toast />
