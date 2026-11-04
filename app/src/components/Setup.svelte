<script lang="ts">
  // Claude — Date 11/03/2026
  // First run, or after disconnecting: the store's address, this device's
  // access key, and what to call the device. Connect checks the address
  // answers and the key is accepted before anything is saved.
  import { load } from '../lib/actions';
  import { api } from '../lib/api';
  import { errorText } from '../lib/format';
  import { LOGO } from '../lib/glyphs';
  import { ui } from '../lib/state.svelte';

  let url = $state(ui.status?.url ?? 'http://127.0.0.1:8080');
  let key = $state('');
  let name = $state(ui.status?.device ?? ui.status?.suggestedName ?? '');
  let busy = $state(false);
  let error = $state('');

  const SPIN = ['|', '/', '-', '\\'];
  let frame = $state(0);
  $effect(() => {
    if (!busy) return;
    const t = setInterval(() => (frame = (frame + 1) % SPIN.length), 120);
    return () => clearInterval(t);
  });

  async function connect(e: SubmitEvent) {
    e.preventDefault();
    if (busy) return;
    busy = true;
    error = '';
    try {
      ui.status = await api.connect(url.trim(), key.trim(), name.trim());
      await load(null);
    } catch (err) {
      error = errorText(err);
    } finally {
      busy = false;
    }
  }
</script>

<div class="drag" data-tauri-drag-region></div>
<div class="screen center">
  <form class="frame double setup" onsubmit={connect}>
    <span class="legend">connect this device</span>
    <pre class="logo">{LOGO}</pre>
    <div class="title">d r a w B r i d g e</div>

    <label class="field">
      <span class="dim">server</span>
      <!-- svelte-ignore a11y_autofocus -->
      <input bind:value={url} spellcheck="false" autocomplete="off" autofocus />
    </label>
    <label class="field">
      <span class="dim">key</span>
      <input type="password" bind:value={key} placeholder="ssk_…" spellcheck="false" autocomplete="off" />
    </label>
    <label class="field">
      <span class="dim">device</span>
      <input bind:value={name} spellcheck="false" autocomplete="off" />
    </label>

    <div class="status">
      {#if busy}
        <span>{SPIN[frame]} connecting</span>
      {:else if error}
        <span class="err">✕ {error}</span>
      {:else}
        <span class="dim"><span class="key">enter</span> to connect</span>
      {/if}
    </div>
    <button type="submit" hidden>connect</button>

    <p class="dim note">
      The key is kept in your Keychain. Plain http:// is allowed only to this computer; anything else must be
      https:// through castle+.
    </p>
  </form>
</div>

<style>
  .drag {
    position: fixed;
    inset: 0 0 auto;
    height: 28px;
  }

  .center {
    align-items: center;
    justify-content: center;
  }

  .setup {
    width: 64ch;
    max-width: 100%;
    padding: var(--cell-h) 3ch;
    display: flex;
    flex-direction: column;
    gap: calc(var(--cell-h) / 2);
  }

  .logo {
    line-height: 1;
    align-self: center;
  }

  .title {
    align-self: center;
    font-weight: 700;
    margin-bottom: calc(var(--cell-h) / 2);
  }

  .field {
    display: grid;
    grid-template-columns: 8ch 1fr;
    align-items: center;
    border-bottom: 1px solid var(--ink);
  }

  .status {
    min-height: var(--cell-h);
    margin-top: calc(var(--cell-h) / 2);
    overflow-wrap: anywhere;
  }

  .note {
    margin: 0;
    line-height: 1.5;
  }
</style>
