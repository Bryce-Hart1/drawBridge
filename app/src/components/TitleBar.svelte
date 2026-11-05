<script lang="ts">
  // Claude — Date 11/03/2026
  // The outer frame's top border: the app's name on the left, this device and
  // the sync state on the right. On macOS it is also the title bar: drag it to
  // move the window.
  import { askDisconnect } from '../lib/actions';
  import { clock, plural } from '../lib/format';
  import { ui } from '../lib/state.svelte';

  const SPIN = ['|', '/', '-', '\\'];
  let frame = $state(0);

  // The spinner turns only while a sync runs, so an idle window has no timers.
  $effect(() => {
    if (ui.status?.sync.state !== 'syncing') return;
    const t = setInterval(() => (frame = (frame + 1) % SPIN.length), 120);
    return () => clearInterval(t);
  });

  const syncText = $derived.by(() => {
    const s = ui.status?.sync;
    if (!s) return '';
    const ro = s.readOnly ? ' · read-only' : '';
    // Changes made here the store doesn't have yet: they go with the next sync that gets through.
    const waiting = s.pending ? ` · ${plural(s.pending, 'change')} waiting` : '';
    switch (s.state) {
      case 'syncing': return `${SPIN[frame]} syncing${waiting}${ro}`;
      case 'offline': return `○ offline${waiting}${ro}`;
      case 'error': return `✕ sync failed${waiting}${ro}`;
      default: return `${s.lastOkMs ? `● synced ${clock(s.lastOkMs)}` : '● ready'}${waiting}${ro}`;
    }
  });
</script>

<div class="titlebar" data-tauri-drag-region>
  <span class="legend left" data-tauri-drag-region>drawBridge</span>
  <span class="legend right" data-tauri-drag-region>
    <button class="device" title="disconnect this device" onclick={askDisconnect}>{ui.status?.device ?? ''}</button>
    <span title={ui.status?.sync.message ?? ''}>&nbsp;&nbsp;{syncText}</span>
  </span>
</div>

<style>
  .titlebar {
    position: absolute;
    left: 0;
    right: 0;
    top: calc(var(--cell-h) / -2 - 1px);
    height: var(--cell-h);
    z-index: 1;
  }

  .legend {
    top: 0;
  }

  .left {
    left: 2ch;
    font-weight: 700;
  }

  /* Clear of the traffic lights. */
  :global(.mac-overlay) .left {
    left: 72px;
  }

  .right {
    left: auto;
    right: 2ch;
  }

  .device:hover {
    text-decoration: underline;
  }
</style>
