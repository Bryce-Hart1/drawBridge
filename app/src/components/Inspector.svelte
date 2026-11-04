<script lang="ts">
  // Claude — Date 11/03/2026
  // The right-hand pane: the selected entry's thumbnail (or its picture), and
  // what it is.
  import { api } from '../lib/api';
  import { hasThumb, sizeLabel, typeLabel } from '../lib/format';
  import { glyph } from '../lib/glyphs';
  import { current, ui } from '../lib/state.svelte';

  const item = $derived(current());
  const where = $derived(`/${ui.listing.path.map((c) => c.name).join('/')}`);
  /** The blob whose thumbnail just failed to load. */
  let missing = $state<string | null>(null);
</script>

<div class="inspector">
  {#if item}
    <div class="art">
      {#if hasThumb(item) && missing !== item.blob}
        {#key item.blob}
          <img class="thumb" src={api.thumbUrl(item.blob!)} alt="" onerror={() => (missing = item.blob)} />
        {/key}
      {:else}
        <pre class="glyph">{glyph(item)}</pre>
      {/if}
    </div>
    <dl>
      <dt>name</dt>
      <dd class="wrap">{item.name}{item.kind === 'folder' ? '/' : ''}</dd>
      <dt>type</dt>
      <dd>{typeLabel(item)}</dd>
      <dt>size</dt>
      <dd>
        {sizeLabel(item)}
        {#if item.kind === 'file' && (item.size ?? 0) >= 1024}<span class="dim">({(item.size ?? 0).toLocaleString()} B)</span>{/if}
      </dd>
      <dt>in</dt>
      <dd class="wrap">{where}</dd>
      <dt>id</dt>
      <dd class="wrap dim">{item.id}</dd>
    </dl>
  {:else}
    <div class="none dim">nothing selected</div>
  {/if}
</div>

<style>
  .inspector {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    gap: calc(var(--cell-h) / 2);
    padding: calc(var(--cell-h) / 2) 1ch;
    overflow: hidden;
  }

  .art {
    height: calc(var(--cell-h) * 10);
    flex: none;
    border: 1px solid var(--ink);
    display: grid;
    place-items: center;
    overflow: hidden;
  }

  .art img {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
  }

  dl {
    display: grid;
    grid-template-columns: 5ch 1fr;
    column-gap: 1ch;
    margin: 0;
  }

  dt {
    color: var(--dim);
  }

  dd {
    margin: 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  dd.wrap {
    white-space: normal;
    overflow-wrap: anywhere;
  }

  .none {
    margin: auto;
  }
</style>
