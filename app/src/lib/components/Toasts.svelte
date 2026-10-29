<script lang="ts">
  import { engine } from "../engine.svelte";
</script>

<div class="toasts" aria-live="polite">
  {#each engine.toasts as t (t.id)}
    <button class="toast {t.level}" onclick={() => engine.dismiss(t.id)}>{t.text}</button>
  {/each}
</div>

<style>
  .toasts { position: fixed; right: 16px; bottom: 16px; display: grid; gap: 8px; z-index: 30; max-width: 380px; }
  .toast { text-align: left; background: var(--panel-2); border-left: 3px solid var(--accent); padding: 8px 12px; }
  .toast.warn { border-left-color: var(--warn); }
  .toast.error { border-left-color: var(--down); }
</style>
