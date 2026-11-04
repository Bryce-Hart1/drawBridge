<script lang="ts">
  // Claude — Date 11/03/2026
  // The folder as tiles: a photo's thumbnail (in gray), or a box-drawing
  // picture of what the entry is. Thumbnails load as tiles scroll into view.
  import { SvelteSet } from 'svelte/reactivity';
  import { open } from '../lib/actions';
  import { api } from '../lib/api';
  import { hasThumb } from '../lib/format';
  import { glyph } from '../lib/glyphs';
  import { ui } from '../lib/state.svelte';
  import Empty from './Empty.svelte';

  let { cols = $bindable(1) }: { cols?: number } = $props();

  let tiles: HTMLDivElement | undefined = $state();
  let width = $state(0);
  /** Blobs whose thumbnail didn't load (none in the store): drawn instead. */
  const missing = new SvelteSet<string>();

  // How many tiles fit in a row, for ↑ and ↓.
  $effect(() => {
    void width;
    if (tiles) cols = Math.max(1, getComputedStyle(tiles).gridTemplateColumns.split(' ').length);
  });

  $effect(() => {
    (tiles?.children[ui.sel] as HTMLElement | undefined)?.scrollIntoView({ block: 'nearest' });
  });
</script>

{#if ui.listing.items.length === 0}
  <Empty />
{:else}
  <div class="scroll" bind:clientWidth={width}>
    <div class="tiles" bind:this={tiles}>
      {#each ui.listing.items as item, i (item.id)}
        {@const sel = i === ui.sel}
        <button class="tile" class:sel onclick={() => (ui.sel = i)} ondblclick={() => open(item)}>
          <div class="art">
            {#if hasThumb(item) && !missing.has(item.blob!)}
              <img
                class="thumb"
                src={api.thumbUrl(item.blob!)}
                alt=""
                loading="lazy"
                decoding="async"
                onerror={() => missing.add(item.blob!)}
              />
            {:else}
              <pre class="glyph">{glyph(item)}</pre>
            {/if}
          </div>
          <div class="label ellipsis" class:inv={sel}>{item.name}{item.kind === 'folder' ? '/' : ''}</div>
        </button>
      {/each}
    </div>
  </div>
{/if}

<style>
  .scroll {
    position: absolute;
    inset: 0;
    overflow-y: auto;
    padding: calc(var(--cell-h) / 2) 1ch;
  }

  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, 22ch);
    gap: calc(var(--cell-h) / 2) 2ch;
  }

  .tile {
    display: flex;
    flex-direction: column;
    width: 22ch;
    border: 1px solid var(--faint);
  }

  /* Selected: a second line around the first, like a double frame. */
  .tile.sel {
    border-color: var(--ink);
    outline: 1px solid var(--ink);
    outline-offset: 2px;
  }

  .art {
    height: calc(var(--cell-h) * 6);
    display: grid;
    place-items: center;
    overflow: hidden;
    border-bottom: 1px solid var(--faint);
  }

  .tile.sel .art {
    border-bottom-color: var(--ink);
  }

  .art img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .label {
    height: var(--cell-h);
    padding: 0 1ch;
    text-align: center;
  }
</style>
