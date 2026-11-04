<script lang="ts">
  // Claude — Date 11/03/2026
  // A question in the bottom line, TUI style:
  //   new folder ▏2026█
  //   delete trips/ and everything in it for every device? [y/N]
  //   3 names already exist: [r]eplace [k]eep both [s]kip
  import { fail } from '../lib/actions';
  import { ui, type Prompt } from '../lib/state.svelte';

  let props: { prompt: Prompt } = $props();
  // Read once: each prompt gets its own instance of this component (Bottom
  // keys it), and the prop itself turns null the moment the prompt is answered.
  // svelte-ignore state_referenced_locally
  const prompt = props.prompt;

  let input: HTMLInputElement | undefined = $state();
  let value = $state(prompt.kind === 'input' ? prompt.value : '');

  $effect(() => {
    if (!input || prompt.kind !== 'input') return;
    input.focus();
    // From the prompt, not `value`, so this runs once rather than per keystroke.
    const [from, to] = prompt.select ?? [0, prompt.value.length];
    input.setSelectionRange(from, to);
  });

  async function finish(then: () => unknown) {
    ui.prompt = null;
    try {
      await then();
    } catch (e) {
      fail(e);
    }
  }

  function onInputKey(e: KeyboardEvent) {
    if (prompt.kind !== 'input') return;
    if (e.key === 'Enter') {
      e.preventDefault();
      const v = value.trim();
      if (v) finish(() => prompt.kind === 'input' && prompt.submit(v));
    } else if (e.key === 'Escape') {
      e.preventDefault();
      ui.prompt = null;
    }
  }

  function onWindowKey(e: KeyboardEvent) {
    if (prompt.kind === 'input') return;
    e.preventDefault();
    if (prompt.kind === 'confirm') {
      if (e.key === 'y' || e.key === 'Y') finish(prompt.yes);
      else ui.prompt = null;
    } else {
      const choice = prompt.choices.find((c) => c.key === e.key.toLowerCase());
      if (choice) finish(choice.pick);
      else if (e.key === 'Escape') ui.prompt = null;
    }
  }
</script>

<svelte:window onkeydown={onWindowKey} />

{#if prompt.kind === 'input'}
  <span class="label">{prompt.label}</span>
  <input bind:this={input} bind:value onkeydown={onInputKey} spellcheck="false" autocomplete="off" />
  <span class="dim">enter ok · esc cancel</span>
{:else if prompt.kind === 'confirm'}
  <span class="ellipsis">{prompt.text}</span>
  <span><span class="key">[y/N]</span></span>
{:else}
  <span class="ellipsis">{prompt.text}</span>
  {#each prompt.choices as c (c.key)}
    <button onclick={() => finish(c.pick)}><span class="key">[{c.key}]</span>{c.label.startsWith(c.key) ? c.label.slice(1) : ` ${c.label}`}</button>
  {/each}
  <span class="dim">esc cancel</span>
{/if}

<style>
  .label {
    font-weight: 700;
  }

  input {
    flex: 1;
    min-width: 10ch;
    border-bottom: 1px dotted var(--dim);
  }
</style>
