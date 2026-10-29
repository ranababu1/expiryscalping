<script lang="ts">
  import { onMount } from "svelte";
  import KillSwitch from "./lib/components/KillSwitch.svelte";
  import OrderTicket from "./lib/components/OrderTicket.svelte";
  import Toasts from "./lib/components/Toasts.svelte";
  import { engine } from "./lib/engine.svelte";
  import { pnl, timeIst } from "./lib/format";
  import Chain from "./lib/screens/Chain.svelte";
  import Connect from "./lib/screens/Connect.svelte";
  import Orders from "./lib/screens/Orders.svelte";
  import Positions from "./lib/screens/Positions.svelte";
  import Risk from "./lib/screens/Risk.svelte";
  import Status from "./lib/screens/Status.svelte";
  import Strategies from "./lib/screens/Strategies.svelte";
  import { ticket } from "./lib/ticket.svelte";
  import { clearToken, loadToken, loadUrl } from "./lib/token";

  const screens = ["Status", "Chain", "Positions", "Orders", "Strategies", "Risk"] as const;
  type Screen = (typeof screens)[number];
  let screen = $state<Screen>("Chain");
  let booting = $state(true);

  onMount(() => {
    void (async () => {
      const t = await loadToken();
      if (t) engine.connect(loadUrl(), t);
      booting = false;
    })();
  });

  const s = $derived(engine.snap.status);
  const total = $derived(engine.snap.positions?.pnl.total ?? null);
  const working = $derived(engine.snap.orders?.orders.filter((o) => o.working).length ?? 0);
  const openPos = $derived(engine.snap.positions?.positions.filter((p) => p.qty !== 0).length ?? 0);
  const live = $derived(engine.conn === "connected");
  const showConnect = $derived(!booting && (engine.conn === "idle" || engine.conn === "unauthorized"));

  async function signOut(): Promise<void> {
    engine.disconnect();
    await clearToken();
  }

  function kill(): void {
    void engine.send({ type: "kill" }, "Kill switch triggered");
  }
</script>

{#if booting}
  <div class="empty">Starting...</div>
{:else if showConnect}
  <Connect reason={engine.conn === "unauthorized" ? `Token refused: ${engine.detail}` : ""} />
{:else}
  <div class="shell">
    <header class="top">
      <strong class="brand">ExpiryScalping</strong>
      <span class="badge paper">{(s?.mode ?? "paper").toUpperCase()}</span>
      {#if s}
        <span class="muted">{s.source}</span>
        <span class="mono">{timeIst(s.engine_time)} IST</span>
        <span class="badge {s.feed.stale || !s.feed.connected ? 'warn' : 'ok'}">{s.feed.connected ? (s.feed.stale ? "Stale" : "Live feed") : "No feed"}</span>
        <span class="muted">p99 {s.latency.p99_us} µs</span>
      {/if}
      <span class="pnl num {total !== null && total > 0 ? 'up' : total !== null && total < 0 ? 'down' : ''}">{pnl(total)}</span>
      <span class="spacer"></span>
      {#if s}
        <button
          class={s.ai_paused ? "primary" : ""}
          disabled={!live}
          onclick={() => s && engine.send({ type: "pause_ai", paused: !s.ai_paused }, s.ai_paused ? "AI resumed" : "AI paused")}
        >
          {s.ai_paused ? "Resume AI" : "Pause AI"}
        </button>
      {/if}
      <KillSwitch triggered={s?.kill.triggered ?? false} disabled={!live} onfire={kill} />
    </header>

    {#if !live}
      <div class="banner" role="status">
        {engine.conn === "connecting" || engine.conn === "authenticating" ? "Connecting to the engine..." : "Disconnected from the engine. Retrying."}
        <span class="muted">{engine.url}</span>
        <span class="muted">Data below may be out of date. The engine keeps enforcing risk on its own.</span>
      </div>
    {/if}

    <div class="body">
      <nav>
        {#each screens as name (name)}
          <button class:active={screen === name} onclick={() => (screen = name)}>
            {name}
            {#if name === "Orders" && working > 0}<span class="badge warn">{working}</span>{/if}
            {#if name === "Positions" && openPos > 0}<span class="badge">{openPos}</span>{/if}
          </button>
        {/each}
        <span class="spacer"></span>
        <button class="small" onclick={signOut}>Sign out</button>
      </nav>
      <main class:stale={!live}>
        {#if screen === "Status"}<Status />
        {:else if screen === "Chain"}<Chain />
        {:else if screen === "Positions"}<Positions />
        {:else if screen === "Orders"}<Orders />
        {:else if screen === "Strategies"}<Strategies />
        {:else}<Risk />{/if}
      </main>
    </div>
  </div>
{/if}

{#if ticket.open}
  {#key ticket.open}<OrderTicket t={ticket.open} />{/key}
{/if}
<Toasts />

<style>
  .shell { height: 100%; display: flex; flex-direction: column; }
  .top { display: flex; align-items: center; gap: 14px; padding: 8px 14px; border-bottom: 1px solid var(--line); background: var(--panel); }
  .brand { letter-spacing: 0.02em; }
  .pnl { font-size: 15px; font-weight: 600; min-width: 100px; }
  .spacer { flex: 1; }
  .banner { padding: 6px 14px; background: #3a2a0c; color: #f0c674; display: flex; gap: 16px; }
  .body { flex: 1; display: flex; min-height: 0; }
  nav { width: 150px; padding: 10px; display: flex; flex-direction: column; gap: 4px; border-right: 1px solid var(--line); }
  nav button { text-align: left; background: none; border-color: transparent; display: flex; justify-content: space-between; }
  nav button.active { background: var(--panel-2); border-color: var(--line); }
  main { flex: 1; padding: 14px; overflow: auto; }
  main.stale { opacity: 0.6; }
</style>
