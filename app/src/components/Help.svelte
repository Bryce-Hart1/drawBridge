<script lang="ts">
  // Claude — Date 11/03/2026
  // Every key, in one box: the browser's, then the player's and viewer's. Any
  // key or click closes it.
  import { ui } from '../lib/state.svelte';

  const KEYS: [string, string][] = [
    ['↑ ↓   j k', 'move'],
    ['enter →', 'open a folder or file'],
    ['← h   ⌫', 'up a folder'],
    ['g', 'list or grid'],
    ['/     ⌘F', 'find in every folder'],
    ['u', 'upload (or drop files in)'],
    ['+', 'new folder'],
    ['n', 'new note'],
    ['l', 'new line diagram (.lin)'],
    ['r', 'rename'],
    ['m', 'move to another folder'],
    ['d', 'delete, for every device'],
    ['o', 'save a copy to this computer'],
    ['e', 'play in IINA, VLC, QuickTime'],
    ['s', 'sync now'],
    ['x', 'cancel the running transfer'],
    [',     ⌘,', 'settings: the cache'],
    ['esc', 'close or cancel'],
    ['space', 'playing: play or pause'],
    ['← →  ⇧', 'playing: seek 5 s (⇧ 30 s)'],
    ['↑ ↓   m', 'playing: volume, mute'],
    ['f     z', 'viewing: fullscreen, 1:1'],
    ['click', 'the device name: disconnect'],
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
    width: 96ch;
  }

  /* Two columns, down then across, where there's room; one, scrolled, where
     there isn't. The smallest window fits two. */
  .list {
    columns: 2 42ch;
    column-gap: 2ch;
    padding: calc(var(--cell-h) / 2) 0;
    overflow-y: auto;
    min-height: 0;
  }

  .list .row {
    break-inside: avoid;
    white-space: nowrap;
  }

  .k {
    width: 9ch;
    flex: none;
  }
</style>
