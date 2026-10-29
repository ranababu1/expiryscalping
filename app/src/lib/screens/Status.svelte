<script lang="ts">
  import { engine } from "../engine.svelte";
  import { dateIst, int, label, pnl, rupees, timeIst } from "../format";

  const s = $derived(engine.snap.status);
  const p = $derived(engine.snap.positions?.pnl ?? null);
</script>

{#if !s}
  <div class="empty">Waiting for the engine status...</div>
{:else}
  <div class="grid">
    <section class="panel card">
      <h3>Engine</h3>
      <dl>
        <dt>Mode</dt><dd><span class="badge paper">{s.mode.toUpperCase()}</span></dd>
        <dt>Source</dt><dd>{s.source}</dd>
        <dt>Version</dt><dd>{s.engine_version}</dd>
        <dt>Device</dt><dd>{engine.snap.hello?.device ?? "-"}</dd>
        <dt>Engine time</dt><dd class="mono">{dateIst(s.engine_time)} {timeIst(s.engine_time)} IST</dd>
        <dt>Market</dt><dd>{s.market_open ? "Open" : "Closed"}</dd>
        {#if s.replay_done}<dt>Replay</dt><dd class="warn">Finished</dd>{/if}
      </dl>
    </section>

    <section class="panel card">
      <h3>Feed</h3>
      <dl>
        <dt>Connected</dt>
        <dd><span class="badge {s.feed.connected ? 'ok' : 'bad'}">{s.feed.connected ? "Yes" : "No"}</span></dd>
        <dt>Data</dt>
        <dd><span class="badge {s.feed.stale ? 'warn' : 'ok'}">{s.feed.stale ? "Stale" : "Fresh"}</span></dd>
        <dt>Last tick</dt><dd>{s.feed.last_tick_age_ms === null ? "-" : `${int(s.feed.last_tick_age_ms)} ms ago`}</dd>
        <dt>Frames</dt><dd class="num">{int(s.feed.frames)}</dd>
        <dt>Kite</dt><dd class="muted">{label(s.kite)}</dd>
      </dl>
    </section>

    <section class="panel card">
      <h3>Tick latency</h3>
      <dl>
        <dt>p50</dt><dd class="num">{s.latency.p50_us === 0 ? "<1" : int(s.latency.p50_us)} µs</dd>
        <dt>p99</dt><dd class="num">{int(s.latency.p99_us)} µs</dd>
        <dt>Samples</dt><dd class="num">{int(s.latency.samples)}</dd>
      </dl>
    </section>

    <section class="panel card">
      <h3>Today</h3>
      <dl>
        <dt>Total P&L</dt>
        <dd class="num {p && p.total > 0 ? 'up' : p && p.total < 0 ? 'down' : ''}">{pnl(p?.total)}</dd>
        <dt>Realised</dt><dd class="num">{pnl(p?.realised)}</dd>
        <dt>Unrealised</dt><dd class="num">{pnl(p?.unrealised)}</dd>
        <dt>Charges</dt><dd class="num">{rupees(p?.charges)}</dd>
        <dt>Funds</dt><dd class="num">{rupees(p?.available_funds)}</dd>
      </dl>
    </section>

    <section class="panel card">
      <h3>Safety</h3>
      <dl>
        <dt>Kill switch</dt>
        <dd>
          {#if s.kill.triggered}
            <span class="badge bad">Triggered</span>
            <span class="muted">{label(s.kill.cause ?? "")} at {timeIst(s.kill.at)}</span>
          {:else}
            <span class="badge ok">Armed</span>
          {/if}
        </dd>
        <dt>AI</dt><dd>{s.ai_paused ? "Paused" : "Running"}</dd>
      </dl>
    </section>
  </div>
{/if}

<style>
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: 12px; }
  .card { padding: 12px 16px; }
  h3 { margin: 0 0 8px; font-size: 13px; color: var(--muted); font-weight: 500; }
  dl { display: grid; grid-template-columns: 110px 1fr; gap: 6px 8px; margin: 0; }
  dt { color: var(--muted); }
  dd { margin: 0; }
</style>
