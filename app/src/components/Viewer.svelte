<script lang="ts">
  // Claude — Date 11/04/2026
  // One file, filling the window: a photo, a PDF, or a song or video with the
  // player line under it. It all loads from the stream server, so a video
  // plays as soon as its first bytes arrive, and a seek fetches only what it
  // needs.
  //
  // Formats WebKit can't play (mkv, avi…) get a panel instead: open the file
  // in IINA, VLC or QuickTime, or save a copy.
  import { onDestroy, onMount } from 'svelte';
  import { fail, openExternal, saveAs } from '../lib/actions';
  import { api } from '../lib/api';
  import { baseMime, human, preview } from '../lib/format';
  import { glyph } from '../lib/glyphs';
  import { ui } from '../lib/state.svelte';
  import type { Item } from '../lib/types';
  import Player from './Player.svelte';

  let props: { item: Item } = $props();
  // Read once: the prop reads ui.overlay, which turns null as the viewer closes.
  // svelte-ignore state_referenced_locally
  const item = { ...props.item };
  const kind = preview(item)!;
  const mime = baseMime(item.mime);
  const media = kind === 'audio' || kind === 'video';
  const playable = !media || document.createElement(kind).canPlayType(mime) !== '';

  let url = $state<string | null>(null);
  let broken = $state(false);
  /** Photos: shown at their own size, scrolled, rather than fitted. */
  let actual = $state(false);
  let fullscreen = $state(false);
  let element: HTMLMediaElement | undefined = $state();
  let player: Player | undefined = $state();

  onMount(async () => {
    if (!playable) return;
    try {
      url = await api.streamUrl(item.id);
    } catch (e) {
      fail(e);
      close();
    }
  });

  onDestroy(() => {
    if (fullscreen) api.setFullscreen(false).catch(() => {});
  });

  function close() {
    ui.overlay = null;
  }

  /** A fresh address, for the player when its stream dropped. */
  async function refresh(): Promise<string | null> {
    try {
      url = await api.streamUrl(item.id);
      return url;
    } catch (e) {
      fail(e);
      return null;
    }
  }

  async function toggleFullscreen() {
    try {
      fullscreen = await api.toggleFullscreen();
    } catch (e) {
      fail(e);
    }
  }

  const keys: Record<string, () => unknown> = {
    Escape: () => (fullscreen ? toggleFullscreen() : close()),
    o: () => saveAs(item),
    e: () => media && openExternal(item),
    z: () => kind === 'image' && (actual = !actual),
    f: () => (kind === 'video' || kind === 'image') && toggleFullscreen(),
    ' ': () => player?.toggle(),
  };

  function onkeydown(e: KeyboardEvent) {
    // Space belongs to the player's own handler.
    if (ui.prompt || e.metaKey || e.ctrlKey || e.altKey || e.key === ' ') return;
    const act = keys[e.key];
    if (!act) return;
    e.preventDefault();
    act();
  }

  type Hint = [label: string, what: string, key: string | null];

  // Clickable too: a PDF frame, once clicked into, keeps the keys to itself.
  const hints: Hint[] = $derived(
    !playable || broken ? [...(media ? [['e', 'open in a player', 'e'] as Hint] : []), ['o', 'save a copy', 'o'], ['esc', 'close', 'Escape']]
    : kind === 'image' ? [['z', actual ? 'fit' : 'actual size', 'z'], ['f', 'fullscreen', 'f'], ['o', 'save', 'o'], ['esc', 'close', 'Escape']]
    : kind === 'pdf' ? [['o', 'save', 'o'], ['esc', 'close', 'Escape']]
    : kind === 'video' ? [['space', 'play', ' '], ['← →', 'seek', null], ['f', 'fullscreen', 'f'], ['e', 'player', 'e'], ['o', 'save', 'o'], ['esc', 'close', 'Escape']]
    : [['space', 'play', ' '], ['← →', 'seek', null], ['↑ ↓', 'volume', null], ['e', 'player', 'e'], ['o', 'save', 'o'], ['esc', 'close', 'Escape']],
  );
</script>

<svelte:window {onkeydown} />

<div class="overlay full">
  <div class="frame double viewer">
    <span class="legend">{item.name} · {human(item.size ?? 0)} · {mime || 'file'}</span>
    <div class="stage">
      {#if !playable || broken}
        <div class="cant">
          <pre class="glyph">{glyph(item)}</pre>
          <p>can't {media ? 'play' : 'show'} {mime || 'this'} here</p>
          {#if media}
            <p class="dim">
              <span class="key">e</span> opens it in IINA, VLC or QuickTime · <span class="key">o</span> saves a copy
            </p>
          {:else}
            <p class="dim"><span class="key">o</span> saves a copy</p>
          {/if}
        </div>
      {:else if !url}
        <span class="dim">loading…</span>
      {:else if kind === 'image'}
        <div class="pic" class:actual>
          <img class="thumb" src={url} alt={item.name} onerror={() => (broken = true)} />
        </div>
      {:else if kind === 'pdf'}
        <iframe src={url} title={item.name}></iframe>
      {:else if kind === 'video'}
        <!-- svelte-ignore a11y_media_has_caption -->
        <video bind:this={element} src={url} autoplay playsinline preload="auto" onclick={() => player?.toggle()}></video>
      {:else}
        <div class="song">
          <pre class="glyph big">{glyph(item)}</pre>
          <div class="ellipsis">{item.name}</div>
        </div>
        <audio bind:this={element} src={url} autoplay preload="auto"></audio>
      {/if}
    </div>
    {#if media && playable && !broken && element}
      <Player bind:this={player} media={element} {refresh} onbroken={() => (broken = true)} />
    {/if}
    <div class="row rule-top dim hints">
      {#each hints as [label, what, key] (label)}
        {#if key}
          <button class="hint" onclick={() => keys[key]()}><span class="key">{label}</span> {what}</button>
        {:else}
          <span><span class="key">{label}</span> {what}</span>
        {/if}
      {/each}
    </div>
  </div>
</div>

<style>
  .stage {
    flex: 1;
    min-height: 0;
    display: grid;
    place-items: center;
    overflow: hidden;
    position: relative;
  }

  /* Flex rather than grid: a flex item's max-height: 100% is of this box. */
  .pic {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: calc(var(--cell-h) / 2) 1ch;
  }

  .pic img {
    max-width: 100%;
    max-height: 100%;
    min-width: 0;
    min-height: 0;
    object-fit: contain;
  }

  .pic.actual {
    overflow: auto;
    align-items: flex-start;
    justify-content: flex-start;
  }

  .pic.actual img {
    max-width: none;
    max-height: none;
  }

  iframe,
  video {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    border: 0;
    background: var(--paper);
  }

  video {
    object-fit: contain;
    background: #000;
  }

  .song,
  .cant {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: calc(var(--cell-h) / 2);
    max-width: 100%;
    padding: 0 2ch;
    text-align: center;
  }

  .cant p {
    margin: 0;
  }

  .big {
    font-size: 200%;
  }

  .hints {
    gap: 2ch;
  }

  .hint:hover {
    text-decoration: underline;
  }
</style>
