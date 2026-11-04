<script lang="ts">
  // Claude — Date 11/03/2026
  // The folder as rows: name, type, size. Only the rows in view are in the
  // page, so a folder of thousands scrolls as fast as one of ten.
  import { open } from '../lib/actions';
  import { sizeLabel, typeLabel } from '../lib/format';
  import { ui } from '../lib/state.svelte';
  import Empty from './Empty.svelte';

  let { pageRows = $bindable(10) }: { pageRows?: number } = $props();

  const ROW = 20; // px, the same as --cell-h
  const SLACK = 10; // rows drawn beyond each edge of the view

  let box: HTMLDivElement | undefined = $state();
  let top = $state(0);
  let height = $state(0);

  const items = $derived(ui.listing.items);
  const first = $derived(Math.max(0, Math.floor(top / ROW) - SLACK));
  const last = $derived(Math.min(items.length, Math.ceil((top + height) / ROW) + SLACK));

  $effect(() => {
    pageRows = Math.max(1, Math.floor(height / ROW) - 1);
  });

  // Keep the selection in view.
  $effect(() => {
    const y = ui.sel * ROW;
    if (!box) return;
    if (y < box.scrollTop) box.scrollTop = y;
    else if (y + ROW > box.scrollTop + box.clientHeight) box.scrollTop = y + ROW - box.clientHeight;
  });
</script>

{#if items.length === 0}
  <Empty />
{:else}
  <div class="list" bind:this={box} bind:clientHeight={height} onscroll={() => (top = box?.scrollTop ?? 0)}>
    <div class="rows" style:height="{items.length * ROW}px">
      {#each items.slice(first, last) as item, i (item.id)}
        {@const at = first + i}
        {@const sel = at === ui.sel}
        <button
          class="row item"
          class:inv={sel}
          style:top="{at * ROW}px"
          onclick={() => (ui.sel = at)}
          ondblclick={() => open(item)}
        >
          <span class="mark">{sel ? '▸' : ' '}</span>
          <span class="name ellipsis" class:folder={item.kind === 'folder'}>{item.name}{item.kind === 'folder' ? '/' : ''}</span>
          <span class="type ellipsis dim">{typeLabel(item)}</span>
          <span class="size">{sizeLabel(item)}</span>
        </button>
      {/each}
    </div>
  </div>
{/if}

<style>
  .list {
    position: absolute;
    inset: 0;
    overflow-y: auto;
    overflow-x: hidden;
  }

  .rows {
    position: relative;
  }

  .item {
    position: absolute;
    left: 0;
    width: 100%;
    gap: 1ch;
  }

  .mark {
    width: 1ch;
    flex: none;
  }

  .name {
    flex: 1;
  }

  .folder {
    font-weight: 700;
  }

  .type {
    width: 24ch;
    flex: none;
  }

  .size {
    width: 11ch;
    flex: none;
    text-align: right;
  }
</style>
