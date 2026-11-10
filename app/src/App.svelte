<script lang="ts">
  // Claude — Date 11/03/2026
  // The window: the setup screen until this device is connected, the browser
  // after. Subscribes once to everything Rust pushes: sync status, index
  // changes, transfer progress, and files dragged over the window.
  import { onMount } from 'svelte';
  import Browser from './components/Browser.svelte';
  import Setup from './components/Setup.svelte';
  import { askUpload, fail, load, reload, reportOriginals } from './lib/actions';
  import { api, type Unlisten } from './lib/api';
  import { ui } from './lib/state.svelte';

  onMount(() => {
    let offs: Unlisten[] = [];
    let gone = false;
    (async () => {
      try {
        const subs = await Promise.all([
          api.onSyncStatus((s) => {
            if (ui.status) ui.status.sync = s;
          }),
          api.onIndexChanged(() => {
            if (ui.status?.connected) reload();
          }),
          api.onTransfer((t) => {
            ui.transfers[t.id] = t;
          }),
          api.onOriginals(reportOriginals),
          api.onDragDrop((e) => {
            if (!ui.status?.connected) return;
            if (e.type === 'enter' || e.type === 'over') {
              ui.dropping = true;
            } else if (e.type === 'leave') {
              ui.dropping = false;
            } else {
              ui.dropping = false;
              askUpload(e.paths);
            }
          }),
        ]);
        if (gone) subs.forEach((off) => off());
        else offs = subs;
        for (const t of await api.transfers()) ui.transfers[t.id] = t;
        ui.status = await api.status();
        if (ui.status.connected) await load(null);
      } catch (e) {
        fail(e);
      }
    })();
    return () => {
      gone = true;
      offs.forEach((off) => off());
    };
  });
</script>

{#if ui.status === null}
  <main class="boot dim">{ui.message?.text ?? 'starting…'}</main>
{:else if !ui.status.connected}
  <Setup />
{:else}
  <Browser />
{/if}

<style>
  .boot {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
  }
</style>
