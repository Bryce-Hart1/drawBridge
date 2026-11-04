<script lang="ts">
  // Claude — Date 11/03/2026
  // Where to move an entry: walk the folders, then choose "move here".
  import { onMount } from 'svelte';
  import { fail, reload, say } from '../lib/actions';
  import { api } from '../lib/api';
  import { ui } from '../lib/state.svelte';
  import type { Crumb, Id, Item } from '../lib/types';

  let props: { item: Item } = $props();
  // Read once: the prop reads ui.overlay, which turns null as the picker closes.
  // svelte-ignore state_referenced_locally
  const item = props.item;

  let dir = $state<Id | null>(null);
  let path = $state<Crumb[]>([]);
  let folders = $state<Item[]>([]);
  /** 0 is "move here"; 1.. are the folders. */
  let sel = $state(0);

  const where = $derived(`/${path.map((c) => c.name).join('/')}`);
  const slash = $derived(item.kind === 'folder' ? '/' : '');

  async function go(to: Id | null, select?: Id) {
    try {
      const l = await api.list(to);
      dir = to;
      path = l.path;
      // It can't go inside itself.
      folders = l.items.filter((i) => i.kind === 'folder' && i.id !== item.id);
      sel = select ? folders.findIndex((f) => f.id === select) + 1 : 0;
    } catch (e) {
      fail(e);
    }
  }

  onMount(() => go(ui.listing.dir));

  async function here() {
    try {
      await api.moveTo(item.id, dir);
      ui.overlay = null;
      await reload();
      say(`moved ${item.name}${slash} to ${where}`);
    } catch (e) {
      fail(e);
    }
  }

  function up() {
    if (path.length) go(path.length > 1 ? path[path.length - 2].id : null, path[path.length - 1].id);
  }

  function onkeydown(e: KeyboardEvent) {
    e.preventDefault();
    const n = folders.length + 1;
    switch (e.key) {
      case 'ArrowDown': case 'j': sel = Math.min(n - 1, sel + 1); break;
      case 'ArrowUp': case 'k': sel = Math.max(0, sel - 1); break;
      case 'Enter': if (sel === 0) here(); else go(folders[sel - 1].id); break;
      case 'ArrowRight': case 'l': if (sel > 0) go(folders[sel - 1].id); break;
      case 'ArrowLeft': case 'h': case 'Backspace': up(); break;
      case 'Escape': ui.overlay = null; break;
    }
  }
</script>

<svelte:window {onkeydown} />

<div class="overlay">
  <div class="frame double picker">
    <span class="legend">move {item.name}{slash} to</span>
    <div class="row path"><span class="ellipsis">{where}</span></div>
    <div class="rows rule-top">
      <button class="row" class:inv={sel === 0} onclick={here}>
        <span>{sel === 0 ? '▸' : ' '}</span><span class="key">[ move here ]</span>
      </button>
      {#each folders as f, i (f.id)}
        <button class="row" class:inv={sel === i + 1} onclick={() => (sel = i + 1)} ondblclick={() => go(f.id)}>
          <span>{sel === i + 1 ? '▸' : ' '}</span><span class="ellipsis">{f.name}/</span>
        </button>
      {/each}
    </div>
    <div class="row rule-top dim">enter choose · → open · ← up · esc cancel</div>
  </div>
</div>

<style>
  .picker {
    width: 56ch;
  }

  .path {
    margin-top: calc(var(--cell-h) / 2);
  }

  .rows {
    overflow-y: auto;
    min-height: calc(var(--cell-h) * 4);
    max-height: calc(var(--cell-h) * 14);
  }

  .rows .row {
    width: 100%;
    gap: 1ch;
  }
</style>
