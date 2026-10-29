<script lang="ts">
  import { engine } from "../engine.svelte";
  import { clearToken, loadUrl, saveToken, saveUrl, secureStorage } from "../token";

  interface Props {
    reason?: string;
  }
  let { reason = "" }: Props = $props();

  let url = $state(loadUrl());
  let token = $state("");
  let remember = $state(true);
  let busy = $state(false);

  async function submit(e: SubmitEvent): Promise<void> {
    e.preventDefault();
    const t = token.trim();
    if (!t || busy) return;
    busy = true;
    saveUrl(url.trim());
    if (remember) await saveToken(t);
    else await clearToken();
    token = "";
    busy = false;
    engine.connect(url.trim(), t);
  }
</script>

<div class="wrap">
  <form class="panel box" onsubmit={submit}>
    <h2>ExpiryScalping</h2>
    <p class="muted">Connect to the engine. Start it with <span class="mono">esd sim</span>; the first run prints a device token once.</p>
    {#if reason}<p class="down">{reason}</p>{/if}
    <label>
      <span>Engine URL</span>
      <input bind:value={url} spellcheck="false" />
    </label>
    <label>
      <span>Device token</span>
      <input type="password" bind:value={token} autocomplete="off" spellcheck="false" />
    </label>
    <label class="check">
      <input type="checkbox" bind:checked={remember} />
      <span>
        Remember on this computer
        <span class="muted">({secureStorage() ? "Windows Credential Manager" : "this session only, browser mode"})</span>
      </span>
    </label>
    <button class="primary" type="submit" disabled={!token.trim() || busy}>Connect</button>
  </form>
</div>

<style>
  .wrap { height: 100%; display: grid; place-items: center; }
  .box { width: 420px; padding: 24px; display: grid; gap: 12px; }
  h2 { margin: 0; font-size: 18px; }
  p { margin: 0; }
  label { display: grid; gap: 4px; }
  label.check { display: flex; gap: 8px; align-items: center; }
</style>
