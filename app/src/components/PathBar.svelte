<script lang="ts">
  // Claude — Date 11/03/2026
  // Where you are, `/ Photos / 2026`, each part clickable, and the list or grid
  // switch.
  import { load } from '../lib/actions';
  import { plural } from '../lib/format';
  import { ui } from '../lib/state.svelte';
</script>

<div class="row pathbar">
  <span class="crumbs ellipsis">
    <button onclick={() => load(null)}>/</button>
    {#each ui.listing.path as crumb, i (crumb.id)}
      <button onclick={() => load(crumb.id)}>&nbsp;{crumb.name}&nbsp;</button>{#if i < ui.listing.path.length - 1}<span class="dim">/</span>{/if}
    {/each}
  </span>
  <span class="spacer"></span>
  <span class="dim">{plural(ui.listing.items.length, 'item')}</span>
  <span class="views">
    <button class:inv={ui.mode === 'list'} onclick={() => (ui.mode = 'list')}>&nbsp;list&nbsp;</button><span class="dim">│</span><button
      class:inv={ui.mode === 'grid'}
      onclick={() => (ui.mode = 'grid')}>&nbsp;grid&nbsp;</button
    >
  </span>
</div>

<style>
  .pathbar {
    margin-top: calc(var(--cell-h) / 2);
  }

  .crumbs button:hover,
  .views button:not(.inv):hover {
    text-decoration: underline;
  }
</style>
