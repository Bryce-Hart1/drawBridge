<script lang="ts">
  // Claude — Date 11/03/2026
  // Shown while files are dragged over the window.
  import { ui } from '../lib/state.svelte';

  const where = $derived(`/${ui.listing.path.map((c) => c.name).join('/')}`);
</script>

<div class="overlay full">
  <div class="frame double drop">
    <span class="legend">upload</span>
    <pre class="arrow">   │
   │
 ╲ │ ╱
  ╲│╱
   ▼</pre>
    <div>drop to upload into</div>
    <div class="key">{where}</div>
    <div class="dim">folders come with everything in them</div>
  </div>
</div>

<style>
  .drop {
    align-items: center;
    justify-content: center;
    gap: calc(var(--cell-h) / 2);
  }

  .arrow {
    line-height: 1.1;
    margin-bottom: var(--cell-h);
  }
</style>
