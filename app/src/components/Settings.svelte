<script lang="ts">
  // Claude — Date 11/04/2026
  // The settings, such as they are: how much of the store this computer keeps
  // on disk (the chunk cache behind the player and viewer), and which store
  // and device this is. ← → change the cap, c clears the caches.
  import { onMount } from 'svelte';
  import { fail, say } from '../lib/actions';
  import { api } from '../lib/api';
  import { bar, human, plural } from '../lib/format';
  import { ui } from '../lib/state.svelte';
  import type { CacheInfo } from '../lib/types';

  const MIB = 1 << 20;
  const GIB = 1 << 30;
  const CAPS = [256 * MIB, 512 * MIB, GIB, 2 * GIB, 5 * GIB, 10 * GIB, 20 * GIB];

  let info = $state<CacheInfo | null>(null);

  onMount(async () => {
    try {
      info = await api.cacheInfo();
    } catch (e) {
      fail(e);
    }
  });

  /** The step nearest the cap, so a cap set some other way still steps. */
  const step = $derived(
    info ? CAPS.reduce((best, c, i) => (Math.abs(c - info!.cap) < Math.abs(CAPS[best] - info!.cap) ? i : best), 0) : 0,
  );

  async function setCap(by: number) {
    if (!info) return;
    const cap = CAPS[Math.max(0, Math.min(CAPS.length - 1, step + by))];
    if (cap === info.cap) return;
    try {
      info = await api.cacheSetCap(cap);
    } catch (e) {
      fail(e);
    }
  }

  function askClear() {
    if (!info) return;
    ui.prompt = {
      kind: 'confirm',
      text: `clear ${human(info.bytes + info.thumbs)} of cached files? they load from the store again`,
      yes: async () => {
        info = await api.cacheClear();
        say('cache cleared');
      },
    };
  }

  function onkeydown(e: KeyboardEvent) {
    if (ui.prompt || e.metaKey || e.ctrlKey || e.altKey) return;
    const act = {
      Escape: () => (ui.overlay = null),
      ArrowLeft: () => setCap(-1),
      h: () => setCap(-1),
      ArrowRight: () => setCap(1),
      l: () => setCap(1),
      c: askClear,
    }[e.key];
    if (!act) return;
    e.preventDefault();
    act();
  }
</script>

<svelte:window {onkeydown} />

<div class="overlay">
  <div class="frame double settings">
    <span class="legend">settings</span>
    <dl>
      <dt>cache</dt>
      <dd>
        {#if info}
          <div>{bar(info.bytes, info.cap, 24)} {human(info.bytes)} of</div>
          <div class="cap">
            <button onclick={() => setCap(-1)} disabled={step === 0} aria-label="smaller">◂</button>
            <span class="inv">&nbsp;{human(info.cap)}&nbsp;</span>
            <button onclick={() => setCap(1)} disabled={step === CAPS.length - 1} aria-label="bigger">▸</button>
          </div>
          <div class="dim">{plural(info.chunks, 'piece')} of files played or viewed</div>
          <div class="dim">thumbnails {human(info.thumbs)}, kept apart</div>
        {:else}
          <span class="dim">…</span>
        {/if}
      </dd>
      <dt>store</dt>
      <dd class="wrap">{ui.status?.url ?? '—'}</dd>
      <dt>device</dt>
      <dd class="wrap">{ui.status?.device ?? '—'}{ui.status?.sync.readOnly ? ' (read-only)' : ''}</dd>
    </dl>
    <div class="row rule-top dim keys">
      <span><span class="key">← →</span> cache size</span>
      <span><button class="link" onclick={askClear}><span class="key">c</span> clear</button></span>
      <span><span class="key">esc</span> close</span>
    </div>
  </div>
</div>

<style>
  .settings {
    width: 64ch;
  }

  dl {
    display: grid;
    grid-template-columns: 8ch 1fr;
    column-gap: 1ch;
    row-gap: calc(var(--cell-h) / 2);
    margin: 0;
    padding: var(--cell-h) 2ch;
  }

  dt {
    color: var(--dim);
  }

  dd {
    margin: 0;
    min-width: 0;
  }

  .wrap {
    overflow-wrap: anywhere;
  }

  .cap {
    display: flex;
    gap: 1ch;
    margin: calc(var(--cell-h) / 4) 0;
  }

  .cap button:disabled {
    color: var(--faint);
  }

  .keys {
    gap: 2ch;
  }

  .link:hover {
    text-decoration: underline;
  }
</style>
