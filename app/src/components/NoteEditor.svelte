<script lang="ts">
  // Claude — Date 11/03/2026
  // A plain-text editor for notes, code and other text files. ⌘S saves: the
  // text is uploaded as the file's new content (or as a new file, the first
  // time), and the next sync shares it. A file over 1 MiB shows its first MiB,
  // read-only.
  import { onMount, tick } from 'svelte';
  import { fail, reload, saveAs, say } from '../lib/actions';
  import { api } from '../lib/api';
  import { human } from '../lib/format';
  import { ui } from '../lib/state.svelte';
  import type { Id } from '../lib/types';

  let props: { note: { id: Id | null; parent: Id | null; name: string; size?: number } } = $props();
  // Read once: the prop reads ui.overlay, which turns null as the editor closes.
  // svelte-ignore state_referenced_locally
  const note = { ...props.note };

  let id = $state(note.id);
  let text = $state('');
  let saved = $state('');
  let loading = $state(note.id !== null);
  /** Only the start of a big file is here: look, don't touch. */
  let partial = $state(false);
  let saving = $state(false);
  let area: HTMLTextAreaElement | undefined = $state();

  const dirty = $derived(text !== saved);
  const lines = $derived(text.split('\n').length);

  onMount(async () => {
    if (id) {
      try {
        const got = await api.noteRead(id);
        text = saved = got.text;
        partial = got.truncated;
      } catch (e) {
        fail(e);
        ui.overlay = null;
        return;
      } finally {
        loading = false;
      }
    }
    // Once the textarea is enabled: a disabled one can't take focus.
    await tick();
    area?.focus();
  });

  async function save() {
    if (saving || loading || partial) return;
    saving = true;
    const body = text;
    try {
      id = await api.noteSave(id ? { id } : { parent: note.parent, name: note.name }, body);
      saved = body;
      say(`saved ${note.name}`);
      await reload();
    } catch (e) {
      fail(e);
    } finally {
      saving = false;
    }
  }

  function close() {
    if (!dirty) {
      ui.overlay = null;
      return;
    }
    area?.blur();
    ui.prompt = {
      kind: 'confirm',
      text: `discard your changes to ${note.name}?`,
      yes: () => (ui.overlay = null),
    };
  }

  function onkeydown(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key === 's') {
      e.preventDefault();
      save();
    } else if (partial && id && e.key === 'o') {
      e.preventDefault();
      saveAs({ id, name: note.name });
    } else if (e.key === 'Escape') {
      e.preventDefault();
      close();
    }
  }
</script>

<div class="overlay full">
  <div class="frame double editor">
    <span class="legend">{note.name}{dirty ? ' ●' : ''}{partial ? ' · read-only' : ''}</span>
    <textarea
      bind:this={area}
      bind:value={text}
      {onkeydown}
      disabled={loading}
      readonly={partial}
      spellcheck="false"
      placeholder={loading ? 'loading…' : ''}
    ></textarea>
    <div class="row rule-top dim">
      {#if partial}
        <span>the first 1 MiB of {human(note.size ?? 0)}</span>
        <span class="spacer"></span>
        <span><span class="key">o</span> save a copy for the rest · <span class="key">esc</span> close</span>
      {:else}
        <span>{saving ? 'saving…' : dirty ? 'modified' : id ? 'saved' : 'new'}</span>
        <span>{lines} {lines === 1 ? 'line' : 'lines'}</span>
        <span class="spacer"></span>
        <span><span class="key">⌘S</span> save · <span class="key">esc</span> close</span>
      {/if}
    </div>
  </div>
</div>

<style>
  textarea {
    flex: 1;
    resize: none;
    padding: calc(var(--cell-h) / 2) 1ch;
    line-height: var(--cell-h);
    white-space: pre-wrap;
    tab-size: 4;
  }
</style>
