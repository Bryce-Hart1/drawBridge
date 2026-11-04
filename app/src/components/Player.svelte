<script lang="ts">
  // Claude — Date 11/04/2026
  // The player line under a video or a song, drawn in text:
  //
  //   ▶ 1:32 ██████▒▒▒░░░░░░░░░░░░ 4:10   vol ▮▮▮▯
  //
  // █ is played, ▒ is loaded ahead, ░ is the rest. Click or drag on the bar to
  // seek. Keys: space plays and pauses, ← → seek 5 s (⇧ for 30 s), ↑ ↓ set the
  // volume, m mutes.
  //
  // If the stream drops (its server stops after a long pause, say), the player
  // asks for the address again once and carries on from the same spot.
  import { onMount } from 'svelte';
  import { duration, scrubBar, volumeBar } from '../lib/format';
  import { ui } from '../lib/state.svelte';

  let {
    media,
    refresh,
    onbroken,
  }: { media: HTMLMediaElement; refresh: () => Promise<string | null>; onbroken: () => void } = $props();

  const SPIN = ['|', '/', '-', '\\'];

  let paused = $state(true);
  let time = $state(0);
  let length = $state(NaN);
  let loaded = $state(0);
  let volume = $state(1);
  let muted = $state(false);
  let waiting = $state(false);
  let frame = $state(0);

  let bar: HTMLSpanElement | undefined = $state();
  let probe: HTMLSpanElement | undefined = $state();
  let barWidth = $state(0);
  const cells = $derived(probe ? Math.max(4, Math.floor(barWidth / probe.getBoundingClientRect().width)) : 20);
  const line = $derived(scrubBar(time, loaded, length, cells));

  let retried = false;
  let dragging = false;

  // The spinner turns only while waiting for bytes.
  $effect(() => {
    if (!waiting || paused) return;
    const t = setInterval(() => (frame = (frame + 1) % SPIN.length), 120);
    return () => clearInterval(t);
  });

  function sync() {
    paused = media.paused;
    time = media.currentTime;
    length = media.duration;
    volume = media.volume;
    muted = media.muted;
    // The end of the loaded stretch that the playhead is in.
    loaded = time;
    for (let i = 0; i < media.buffered.length; i++) {
      if (media.buffered.start(i) <= time + 0.5 && media.buffered.end(i) >= time) loaded = media.buffered.end(i);
    }
  }

  async function onerror() {
    if (retried) return onbroken();
    retried = true;
    const at = media.currentTime;
    const resume = !media.paused || media.autoplay;
    const url = await refresh();
    if (!url) return onbroken();
    media.src = url;
    media.load();
    media.currentTime = at;
    if (resume) media.play().catch(() => {});
  }

  onMount(() => {
    const events = ['timeupdate', 'durationchange', 'progress', 'play', 'pause', 'volumechange', 'seeked', 'loadedmetadata'];
    const busy = () => (waiting = true);
    const ready = () => {
      waiting = false;
      // Playing again: a later drop may retry again.
      retried = false;
    };
    // The element as mounted: by teardown the prop may already be gone.
    const el = media;
    for (const ev of events) el.addEventListener(ev, sync);
    el.addEventListener('waiting', busy);
    el.addEventListener('seeking', busy);
    el.addEventListener('playing', ready);
    el.addEventListener('canplay', ready);
    el.addEventListener('error', onerror);
    sync();
    return () => {
      for (const ev of events) el.removeEventListener(ev, sync);
      el.removeEventListener('waiting', busy);
      el.removeEventListener('seeking', busy);
      el.removeEventListener('playing', ready);
      el.removeEventListener('canplay', ready);
      el.removeEventListener('error', onerror);
    };
  });

  export function toggle() {
    if (media.paused) media.play().catch(() => {});
    else media.pause();
  }

  function seekBy(seconds: number) {
    if (Number.isFinite(media.duration)) media.currentTime = Math.max(0, Math.min(media.duration, media.currentTime + seconds));
  }

  function seekTo(e: PointerEvent) {
    if (!bar || !Number.isFinite(media.duration)) return;
    const r = bar.getBoundingClientRect();
    media.currentTime = Math.max(0, Math.min(1, (e.clientX - r.left) / r.width)) * media.duration;
  }

  function onkeydown(e: KeyboardEvent) {
    if (ui.prompt || e.metaKey || e.ctrlKey || e.altKey) return;
    const big = e.shiftKey ? 30 : 5;
    switch (e.key) {
      case ' ': toggle(); break;
      case 'ArrowLeft': seekBy(-big); break;
      case 'ArrowRight': seekBy(big); break;
      case 'ArrowUp': media.volume = Math.min(1, Math.round((media.volume + 0.25) * 4) / 4); media.muted = false; break;
      case 'ArrowDown': media.volume = Math.max(0, Math.round((media.volume - 0.25) * 4) / 4); break;
      case 'm': media.muted = !media.muted; break;
      default: return;
    }
    e.preventDefault();
  }
</script>

<svelte:window {onkeydown} />

<div class="row rule-top player">
  <button class="state" title={paused ? 'play (space)' : 'pause (space)'} onclick={toggle}>
    {paused ? '▶' : waiting ? SPIN[frame] : '‖'}
  </button>
  <span class="time">{duration(time)}</span>
  <span
    class="bar"
    role="slider"
    tabindex="-1"
    aria-label="position"
    aria-valuemin={0}
    aria-valuemax={Number.isFinite(length) ? Math.floor(length) : 0}
    aria-valuenow={Math.floor(time)}
    bind:this={bar}
    bind:clientWidth={barWidth}
    onpointerdown={(e) => {
      dragging = true;
      bar?.setPointerCapture(e.pointerId);
      seekTo(e);
    }}
    onpointermove={(e) => dragging && seekTo(e)}
    onpointerup={() => (dragging = false)}
  >{line}</span>
  <span class="time">{duration(length)}</span>
  <button class="vol" title="mute (m)" onclick={() => (media.muted = !media.muted)}>
    <span class="dim">vol</span> {volumeBar(volume, muted)}
  </button>
  <span class="probe" bind:this={probe} aria-hidden="true">0</span>
</div>

<style>
  .player {
    gap: 1ch;
    user-select: none;
  }

  .state {
    width: 1ch;
    flex: none;
  }

  .time {
    flex: none;
    min-width: 4ch;
    text-align: right;
  }

  .bar {
    flex: 1;
    min-width: 4ch;
    overflow: hidden;
    white-space: pre;
    cursor: pointer;
  }

  .vol {
    flex: none;
    width: 10ch;
    text-align: left;
    margin-left: 1ch;
  }

  .probe {
    position: absolute;
    visibility: hidden;
  }
</style>
