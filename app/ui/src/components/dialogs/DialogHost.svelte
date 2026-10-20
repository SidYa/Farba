<script lang="ts">
  // Renders the open dialogs (lib/dialogs.svelte.ts), the last one on top.
  import { dialogs } from "../../lib/dialogs.svelte";
</script>

{#each dialogs.stack as d (d.id)}
  <d.component {...d.props} close={d.close} />
{/each}
