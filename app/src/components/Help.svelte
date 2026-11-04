<script lang="ts">
  // Claude — Date 11/03/2026
  // Every key, in one box. Any key or click closes it.
  import { ui } from '../lib/state.svelte';

  const KEYS: [string, string][] = [
    ['↑ ↓   j k', 'move'],
    ['enter → l', 'open: folder, note, or save a copy'],
    ['← h   ⌫', 'up a folder'],
    ['g', 'list or grid'],
    ['/     ⌘F', 'find anything, in every folder'],
    ['u', 'upload files (or drop them anywhere)'],
    ['+', 'new folder'],
    ['n', 'new note'],
    ['r', 'rename'],
    ['m', 'move to another folder'],
    ['d', 'delete, for every device'],
    ['o', 'save a copy to this computer'],
    ['s', 'sync now'],
    ['x', 'cancel the running transfer'],
    ['esc', 'close or cancel'],
    ['click', 'the device name, top right, to disconnect'],
  ];

  const close = () => (ui.overlay = null);
</script>

<svelte:window
  onkeydown={(e) => {
    e.preventDefault();
    close();
  }}
/>

<div class="overlay">
  <button class="frame double help" onclick={close}>
    <span class="legend">keys</span>
    <div class="list">
      {#each KEYS as [k, what] (k)}
        <div class="row"><span class="k key">{k}</span><span>{what}</span></div>
      {/each}
    </div>
    <div class="row rule-top dim">any key closes</div>
  </button>
</div>

<style>
  .help {
    width: 60ch;
  }

  .list {
    padding: calc(var(--cell-h) / 2) 0;
  }

  .k {
    width: 10ch;
    flex: none;
  }
</style>
