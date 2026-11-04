<script lang="ts">
  // Claude — Date 11/03/2026
  // Find anything by name, in every folder. Searches this device's copy of the
  // index, so it answers as you type and works offline. Enter opens the folder
  // the result is in, with the result selected.
  import { fail, load } from '../lib/actions';
  import { api } from '../lib/api';
  import { sizeLabel } from '../lib/format';
  import { ui } from '../lib/state.svelte';
  import type { Hit } from '../lib/types';

  let query = $state('');
  let hits = $state<Hit[]>([]);
  let sel = $state(0);
  let asked = 0;

  $effect(() => {
    const q = query.trim();
    const n = ++asked;
    if (!q) {
      hits = [];
      return;
    }
    const t = setTimeout(async () => {
      try {
        const found = await api.search(q);
        // A slower answer to an older query doesn't overwrite a newer one.
        if (n === asked) {
          hits = found;
          sel = 0;
        }
      } catch (e) {
        fail(e);
      }
    }, 60);
    return () => clearTimeout(t);
  });

  function go(hit: Hit) {
    ui.overlay = null;
    load(hit.parent, hit.item.id);
  }

  function onkeydown(e: KeyboardEvent) {
    switch (e.key) {
      case 'ArrowDown': e.preventDefault(); sel = Math.min(hits.length - 1, sel + 1); break;
      case 'ArrowUp': e.preventDefault(); sel = Math.max(0, sel - 1); break;
      case 'Enter': e.preventDefault(); if (hits[sel]) go(hits[sel]); break;
      case 'Escape': e.preventDefault(); ui.overlay = null; break;
    }
  }
</script>

<div class="overlay">
  <div class="frame double finder">
    <span class="legend">find</span>
    <label class="row query">
      <span class="key">/</span>
      <!-- svelte-ignore a11y_autofocus -->
      <input bind:value={query} {onkeydown} autofocus spellcheck="false" autocomplete="off" placeholder="a name, or part of a path" />
    </label>
    <div class="rows rule-top">
      {#each hits as hit, i (hit.item.id)}
        <button class="row" class:inv={i === sel} onclick={() => (sel = i)} ondblclick={() => go(hit)}>
          <span>{i === sel ? '▸' : ' '}</span>
          <span class="name ellipsis">{hit.item.name}{hit.item.kind === 'folder' ? '/' : ''}</span>
          <span class="where ellipsis dim">{hit.path}</span>
          <span class="size">{sizeLabel(hit.item)}</span>
        </button>
      {:else}
        <div class="row dim">{query.trim() ? 'nothing by that name' : 'type to search every folder'}</div>
      {/each}
    </div>
    <div class="row rule-top dim">↑↓ choose · enter go there · esc close</div>
  </div>
</div>

<style>
  .finder {
    width: 84ch;
    height: 100%;
  }

  .query {
    margin-top: calc(var(--cell-h) / 2);
    gap: 1ch;
  }

  .query input {
    flex: 1;
  }

  .rows {
    flex: 1;
    overflow-y: auto;
  }

  .rows .row {
    width: 100%;
    gap: 1ch;
  }

  .name {
    flex: 1;
  }

  .where {
    width: 30ch;
    flex: none;
  }

  .size {
    width: 11ch;
    flex: none;
    text-align: right;
  }
</style>
