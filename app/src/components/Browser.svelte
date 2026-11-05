<script lang="ts">
  // Claude — Date 11/03/2026
  // The main screen, laid out like a terminal UI:
  //
  //   ┌─ drawBridge ──────────────── device ● synced ─┐   TitleBar
  //   │ / Photos / 2026                    list│grid │   PathBar
  //   ├──────────────────────────────┬────────────────┤
  //   │ listing or grid              │ inspector      │
  //   ├──────────────────────────────┴────────────────┤
  //   │ ↑ upload in progress                          │   TransferLine
  //   ├───────────────────────────────────────────────┤
  //   │ [u]pload [+]folder …  or a prompt             │   Bottom
  //   └───────────────────────────────────────────────┘
  //
  // Keys go to whatever is on top. An overlay or a prompt handles its own;
  // otherwise they arrive here as actions (keys.ts).
  import * as act from '../lib/actions';
  import { actionFor, type Action } from '../lib/keys';
  import { current, ui } from '../lib/state.svelte';
  import Bottom from './Bottom.svelte';
  import DropZone from './DropZone.svelte';
  import Finder from './Finder.svelte';
  import FolderPicker from './FolderPicker.svelte';
  import Grid from './Grid.svelte';
  import Help from './Help.svelte';
  import Inspector from './Inspector.svelte';
  import LinEditor from './LinEditor.svelte';
  import Listing from './Listing.svelte';
  import NoteEditor from './NoteEditor.svelte';
  import PathBar from './PathBar.svelte';
  import Settings from './Settings.svelte';
  import TitleBar from './TitleBar.svelte';
  import TransferLine from './TransferLine.svelte';
  import Viewer from './Viewer.svelte';

  /** Tiles per row in the grid; rows per page in the list. Both measured by those views. */
  let cols = $state(1);
  let pageRows = $state(10);

  function move(by: number) {
    const n = ui.listing.items.length;
    if (n > 0) ui.sel = Math.max(0, Math.min(n - 1, ui.sel + by));
  }

  function run(a: Action) {
    const item = current();
    const grid = ui.mode === 'grid';
    switch (a) {
      case 'up': return move(grid ? -cols : -1);
      case 'down': return move(grid ? cols : 1);
      case 'left': return grid ? move(-1) : act.back();
      case 'right': return grid ? move(1) : item && act.open(item);
      case 'open': return item && act.open(item);
      case 'back': return act.back();
      case 'top': return move(-Infinity);
      case 'bottom': return move(Infinity);
      case 'pageUp': return move(-(grid ? cols * 3 : pageRows));
      case 'pageDown': return move(grid ? cols * 3 : pageRows);
      case 'upload': return act.pickAndUpload();
      case 'mkdir': return act.askMkdir();
      case 'note': return act.askNote();
      case 'diagram': return act.askDiagram();
      case 'rename': return item && act.askRename(item);
      case 'move':
        if (item) ui.overlay = { kind: 'move', item };
        return;
      case 'delete': return item && act.askDelete(item);
      case 'save': return item?.kind === 'file' && act.saveAs(item);
      case 'find':
        ui.overlay = { kind: 'find' };
        return;
      case 'grid':
        ui.mode = grid ? 'list' : 'grid';
        return;
      case 'sync': return act.syncNow();
      case 'cancel': return act.cancelTransfer();
      case 'help':
        ui.overlay = { kind: 'help' };
        return;
      case 'external': return item && act.openExternal(item);
      case 'settings':
        ui.overlay = { kind: 'settings' };
        return;
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (ui.overlay || ui.prompt) return;
    const target = e.target as HTMLElement | null;
    if (target?.tagName === 'INPUT' || target?.tagName === 'TEXTAREA') return;
    const a = actionFor(e);
    if (!a) return;
    e.preventDefault();
    ui.message = null;
    run(a);
  }
</script>

<svelte:window {onkeydown} />

<div class="drag" data-tauri-drag-region></div>
<div class="screen">
  <section class="frame outer">
    <TitleBar />
    <PathBar />
    <div class="body rule-top">
      <div class="main">
        {#if ui.mode === 'list'}
          <Listing bind:pageRows />
        {:else}
          <Grid bind:cols />
        {/if}
      </div>
      <aside class="side">
        <Inspector />
      </aside>
    </div>
    <TransferLine />
    <Bottom {run} />
  </section>

  {#if ui.overlay?.kind === 'help'}
    <Help />
  {:else if ui.overlay?.kind === 'find'}
    <Finder />
  {:else if ui.overlay?.kind === 'move'}
    <FolderPicker item={ui.overlay.item} />
  {:else if ui.overlay?.kind === 'note'}
    {#key ui.overlay}
      <NoteEditor note={ui.overlay} />
    {/key}
  {:else if ui.overlay?.kind === 'lin'}
    {#key ui.overlay}
      <LinEditor lin={ui.overlay} />
    {/key}
  {:else if ui.overlay?.kind === 'view'}
    {#key ui.overlay}
      <Viewer item={ui.overlay.item} />
    {/key}
  {:else if ui.overlay?.kind === 'settings'}
    <Settings />
  {/if}
  {#if ui.dropping}
    <DropZone />
  {/if}
</div>

<style>
  /* The strip above the frame: with no title bar, this is where the window is dragged from. */
  .drag {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    height: calc(var(--top) + var(--cell-h) / 2);
    z-index: 0;
  }

  .outer {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }

  .body {
    flex: 1;
    display: flex;
    min-height: 0;
  }

  .main {
    flex: 1;
    min-width: 0;
    position: relative;
  }

  .side {
    width: 38ch;
    flex: none;
    border-left: 1px solid var(--ink);
    position: relative;
  }

  @media (max-width: 860px) {
    .side {
      display: none;
    }
  }
</style>
