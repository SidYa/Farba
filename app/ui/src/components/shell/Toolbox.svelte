<script lang="ts">
  // One-column toolbox in GIMP's order. By default every tool has its own button, with faint
  // three-dot dividers between groups of similar tools. With Settings > "Group similar
  // tools" (owner decision 2026-10-12) similar tools share a button (TOOL_GROUPS): it shows
  // the last chosen tool of its group and has a corner mark; holding the left button opens
  // the group to the right, where a tool is picked by releasing over it or by clicking it.
  // A plain click selects the shown tool; right click selects it and opens its options.
  import Icon from "../Icon.svelte";
  import ToolFlyout from "./ToolFlyout.svelte";
  import ToolPopover from "./ToolPopover.svelte";
  import { shortcutOf } from "../../lib/actions";
  import { TOOL_GROUPS, toolDef, tools } from "../../lib/tools.svelte";

  /** How long the button must be held to open its group. */
  const HOLD_MS = 300;

  let popover = $state<{ toolId: string; anchor: DOMRect } | undefined>();
  let flyout = $state<{ group: string[]; anchor: DOMRect } | undefined>();
  let holdTimer: ReturnType<typeof setTimeout> | undefined;

  function openOptions(e: MouseEvent, id: string) {
    e.preventDefault();
    flyout = undefined;
    tools.select(id);
    const anchor = (e.currentTarget as HTMLElement).getBoundingClientRect();
    popover = popover?.toolId === id ? undefined : { toolId: id, anchor };
  }

  function startHold(e: PointerEvent, group: string[]) {
    if (e.button !== 0 || group.length < 2) return;
    const button = e.currentTarget as HTMLElement;
    clearTimeout(holdTimer);
    holdTimer = setTimeout(() => {
      popover = undefined;
      flyout = { group, anchor: button.getBoundingClientRect() };
    }, HOLD_MS);
  }

  function cancelHold() {
    clearTimeout(holdTimer);
  }

  // Grouped: one button per group; otherwise one per tool (each a group of one).
  let buttons = $derived(tools.grouped ? TOOL_GROUPS : TOOL_GROUPS.flat().map((id) => [id]));
  /** First tool of every group: a divider goes above it when not grouped. */
  const starts = new Set(TOOL_GROUPS.map((g) => g[0]!));

  function tooltip(id: string, group: string[]): string {
    const t = toolDef(id);
    let text = `${t.name}\nShortcut key: ${shortcutOf(`tool-${id}`)}\n\n${t.help}\n\nRight click: tool options`;
    if (group.length > 1) {
      const others = group.filter((g) => g !== id).map((g) => toolDef(g).name);
      text += `\nHold: ${others.join(", ")}`;
    }
    return text;
  }
</script>

<nav class="toolbox" aria-label="Tools">
  {#each buttons as group, i (group[0])}
    {@const id = tools.shown(group)}
    {#if !tools.grouped && i > 0 && starts.has(group[0]!)}
      <span class="divider"><Icon name="more_horiz" size={16} /></span>
    {/if}
    <button
      class="tool"
      class:on={tools.active === id}
      class:group={group.length > 1}
      title={tooltip(id, group)}
      aria-pressed={tools.active === id}
      aria-haspopup={group.length > 1 ? "menu" : undefined}
      onpointerdown={(e) => startHold(e, group)}
      onpointerup={cancelHold}
      onpointerleave={cancelHold}
      onclick={() => tools.select(id)}
      oncontextmenu={(e) => openOptions(e, id)}
    >
      <Icon name={toolDef(id).icon} size={20} />
    </button>
  {/each}
</nav>

{#if flyout}
  <ToolFlyout group={flyout.group} anchor={flyout.anchor} onclose={() => (flyout = undefined)} />
{/if}

{#if popover}
  {#key popover.toolId}
    <ToolPopover
      toolId={popover.toolId}
      anchor={popover.anchor}
      onclose={() => (popover = undefined)}
    />
  {/key}
{/if}

<style>
  .toolbox {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    width: var(--toolbox-w);
    padding: 4px 0;
    background: var(--bg-panel);
    border-right: 1px solid var(--border);
    overflow-y: auto;
    scrollbar-width: none;
  }
  .tool {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 28px;
    flex: none;
    padding: 0;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
  }
  /* Three dots between groups, like the header bar dividers (theme foreground, faint). */
  .divider {
    flex: none;
    display: inline-flex;
    height: 10px;
    align-items: center;
    opacity: 0.35;
  }
  .tool:hover {
    background: var(--hover);
  }
  .tool.on {
    background: var(--accent-soft);
    color: var(--fg);
    box-shadow: inset 0 0 0 1px var(--accent);
  }
  /* Corner mark: the button holds more tools. */
  .tool.group::after {
    content: "";
    position: absolute;
    right: 2px;
    bottom: 2px;
    border-style: solid;
    border-width: 0 0 4px 4px;
    border-color: transparent transparent var(--fg-dim) transparent;
  }
</style>
