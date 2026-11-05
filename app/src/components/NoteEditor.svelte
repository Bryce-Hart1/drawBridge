<script lang="ts">
  // Claude — Date 11/03/2026
  // A plain-text editor for notes, code and other text files. ⌘S saves: the
  // text is uploaded as the file's new content (or as a new file, the first
  // time), and the next sync shares it. A file over 1 MiB shows its first MiB,
  // read-only.
  //
  // Code is coloured (lib/syntax.ts): the coloured copy sits under a
  // textarea whose own text is transparent, scrolled in step with it, so
  // editing is still plain textarea editing. Code doesn't wrap, so the two
  // always break lines in the same places.
  import { onMount, tick } from 'svelte';
  import { fail, reload, saveAs, say } from '../lib/actions';
  import { api } from '../lib/api';
  import { human } from '../lib/format';
  import { highlight, langFor } from '../lib/syntax';
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

  /** Past this, colouring each keystroke would lag; the text shows plain. */
  const COLOUR_LIMIT = 256 << 10;
  const lang = langFor(note.name);
  let pre: HTMLPreElement | undefined = $state();
  // The trailing newline keeps a last empty line as tall as the textarea's.
  const coloured = $derived(lang && text.length <= COLOUR_LIMIT ? `${highlight(text, lang)}\n` : null);

  /** The coloured copy follows the textarea: the same visible box (its
   *  scrollbars excluded, since the copy has none) and the same scroll. */
  function follow() {
    if (!pre || !area) return;
    pre.style.right = `${area.offsetWidth - area.clientWidth}px`;
    pre.style.bottom = `${area.offsetHeight - area.clientHeight}px`;
    pre.scrollTop = area.scrollTop;
    pre.scrollLeft = area.scrollLeft;
  }

  $effect(() => {
    void coloured;
    follow();
  });

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
      // Offline, the text stays here; notes aren't queued like uploads.
      if ((e as { kind?: string }).kind === 'offline') say(`can't reach the store: ${note.name} isn't saved yet; ⌘S again once it's back`, true);
      else fail(e);
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

<svelte:window onresize={follow} />

<div class="overlay full">
  <div class="frame double editor">
    <span class="legend">{note.name}{dirty ? ' ●' : ''}{partial ? ' · read-only' : ''}</span>
    <div class="body" class:code={coloured !== null}>
      {#if coloured !== null}
        <!-- Escaped in highlight(): every character of the text is an entity or plain. -->
        <pre class="paint" aria-hidden="true" bind:this={pre}>{@html coloured}</pre>
      {/if}
      <textarea
        bind:this={area}
        bind:value={text}
        {onkeydown}
        onscroll={follow}
        disabled={loading}
        readonly={partial}
        spellcheck="false"
        wrap={coloured !== null ? 'off' : 'soft'}
        placeholder={loading ? 'loading…' : ''}
      ></textarea>
    </div>
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
  .body {
    flex: 1;
    min-height: 0;
    display: flex;
    position: relative;
  }

  textarea,
  .paint {
    padding: calc(var(--cell-h) / 2) 1ch;
    line-height: var(--cell-h);
    tab-size: 4;
  }

  textarea {
    flex: 1;
    resize: none;
    white-space: pre-wrap;
  }

  /* Code: the textarea on top shows only its caret and selection; the
     coloured copy underneath shows the text. */
  .code textarea,
  .paint {
    position: absolute;
    inset: 0;
    white-space: pre;
  }

  .code textarea {
    color: transparent;
    overflow: auto;
    z-index: 1;
  }

  .code textarea::placeholder {
    color: var(--dim);
  }

  .paint {
    overflow: hidden;
    pointer-events: none;
    color: var(--ink);
  }
</style>
