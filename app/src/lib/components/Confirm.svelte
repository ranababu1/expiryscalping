<script lang="ts">
  interface Props {
    title: string;
    body: string;
    confirmText: string;
    onconfirm: () => void;
    oncancel: () => void;
  }
  let { title, body, confirmText, onconfirm, oncancel }: Props = $props();
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && oncancel()} />
<div class="backdrop" role="presentation" onclick={oncancel}></div>
<div class="dialog panel" role="alertdialog" aria-label={title}>
  <strong>{title}</strong>
  <p>{body}</p>
  <footer>
    <button onclick={oncancel}>Cancel</button>
    <button class="danger" onclick={onconfirm}>{confirmText}</button>
  </footer>
</div>

<style>
  .backdrop { position: fixed; inset: 0; background: rgb(0 0 0 / 0.5); z-index: 20; }
  .dialog { position: fixed; top: 40%; left: 50%; transform: translate(-50%, -50%); width: 360px; padding: 16px; z-index: 21; }
  p { color: var(--muted); }
  footer { display: flex; justify-content: flex-end; gap: 8px; }
</style>
