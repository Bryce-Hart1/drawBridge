<script lang="ts">
  // Claude — Date 11/03/2026
  // One line for transfers: the one running, with a bar and its speed, and how
  // many wait behind it. Otherwise, how the last one ended.
  import { bar, human, percent } from '../lib/format';
  import { ui } from '../lib/state.svelte';

  const all = $derived(Object.values(ui.transfers).sort((a, b) => a.id - b.id));
  const active = $derived(all.find((t) => t.state === 'running'));
  const queued = $derived(all.filter((t) => t.state === 'queued').length);
  const last = $derived(all.findLast((t) => t.state !== 'running' && t.state !== 'queued'));

  // Bytes a second, smoothed over the progress events (one per chunk).
  let rate = $state(0);
  let prev: { id: number; done: number; at: number } | null = null;
  $effect(() => {
    const t = active;
    if (!t) {
      prev = null;
      rate = 0;
      return;
    }
    const now = performance.now();
    if (prev?.id === t.id && t.done > prev.done) {
      const r = (t.done - prev.done) / ((now - prev.at) / 1000);
      rate = rate ? rate * 0.6 + r * 0.4 : r;
    }
    if (prev?.id !== t.id || t.done !== prev.done) prev = { id: t.id, done: t.done, at: now };
  });
</script>

<div class="row rule-top">
  {#if active}
    <span>{active.dir === 'up' ? '↑' : '↓'}</span>
    <span class="name ellipsis">{active.name}</span>
    <span>{bar(active.done, active.total, 20)}</span>
    <span class="pct">{percent(active.done, active.total)}</span>
    <span class="dim">{rate ? `${human(Math.round(rate))}/s` : ''}</span>
    <span class="spacer"></span>
    {#if queued}<span class="dim">+{queued} queued</span>{/if}
    <span class="dim"><span class="key">[x]</span> cancel</span>
  {:else if last}
    {#if last.state === 'done'}
      <span>✓ {last.dir === 'up' ? 'uploaded' : 'saved'} {last.name}</span>
    {:else if last.state === 'failed'}
      <span class="err">✕ {last.name}: {last.error}</span>
    {:else}
      <span class="dim">– {last.name} stopped</span>
    {/if}
    <span class="spacer"></span>
    {#if queued}<span class="dim">+{queued} queued</span>{/if}
  {:else}
    <span class="dim">no transfers</span>
  {/if}
</div>

<style>
  .name {
    max-width: 40ch;
  }

  .pct {
    width: 4ch;
    text-align: right;
  }
</style>
