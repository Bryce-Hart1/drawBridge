<script lang="ts">
  // Claude — Date 11/03/2026
  // The last line: the key hints, or a prompt in their place, or a message for
  // a few seconds. The hints can be clicked too.
  import { HINTS, hintRest, type Action } from '../lib/keys';
  import { ui } from '../lib/state.svelte';
  import Prompt from './Prompt.svelte';

  let { run }: { run: (a: Action) => void } = $props();
</script>

<div class="row rule-top bottom">
  {#if ui.prompt}
    {#key ui.prompt}
      <Prompt prompt={ui.prompt} />
    {/key}
  {:else if ui.message}
    <span class:err={ui.message.error} class="ellipsis">{ui.message.error ? '✕ ' : ''}{ui.message.text}</span>
  {:else}
    {#each HINTS as h (h.action)}
      <button class="hint" onclick={() => run(h.action)}><span class="key">[{h.key}]</span>{hintRest(h)}</button>
    {/each}
  {/if}
</div>

<style>
  .bottom {
    gap: 1ch;
  }

  .hint:hover {
    text-decoration: underline;
  }
</style>
