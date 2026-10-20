<script lang="ts">
  // Progress of an add-on action (e.g. downloading a model, then working). Cancel stops it;
  // the dialog closes itself when the action ends.
  import Dialog from "./Dialog.svelte";
  import { addons, cancelAddon } from "../../lib/addons.svelte";
  import type { DialogProps } from "../../lib/dialogs.svelte";

  let { close }: DialogProps<boolean> = $props();

  let r = $derived(addons.running);

  $effect(() => {
    if (!addons.running) close(true);
  });
</script>

<Dialog title={r?.title ?? ""} width={360} onsubmit={() => {}} oncancel={cancelAddon}>
  <div class="text">{r?.text ?? ""}</div>
  <div class="bar" class:busy={(r?.fraction ?? -1) < 0}>
    <div class="fill" style:width="{(r?.fraction ?? -1) < 0 ? 30 : r!.fraction * 100}%"></div>
  </div>
  {#if r && r.fraction >= 0}
    <div class="pct">{Math.round(r.fraction * 100)}%</div>
  {/if}
  {#snippet buttons()}
    <button type="button" onclick={cancelAddon}>Cancel</button>
  {/snippet}
</Dialog>

<style>
  .bar {
    position: relative;
    height: 6px;
    border-radius: 3px;
    background: var(--border-soft);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    border-radius: 3px;
    background: var(--accent);
    transition: width 0.15s;
  }
  .bar.busy .fill {
    position: absolute;
    animation: slide 1.2s ease-in-out infinite;
  }
  @keyframes slide {
    from {
      left: -30%;
    }
    to {
      left: 100%;
    }
  }
  .pct {
    color: var(--fg-dim);
    font-size: 11px;
    text-align: right;
  }
</style>
