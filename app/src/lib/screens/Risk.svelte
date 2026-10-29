<script lang="ts">
  import { engine } from "../engine.svelte";
  import { label, pnl, rupees, timeIst } from "../format";
  import { FIELDS, fromDraft, toDraft, type Draft } from "../riskform";

  const r = $derived(engine.snap.risk);
  let editing = $state(false);
  let draft = $state<Draft | null>(null);
  let confirmLoosen = $state(false);
  let error = $state("");
  let busy = $state(false);

  function edit(): void {
    if (!r) return;
    draft = toDraft(r.limits);
    confirmLoosen = false;
    error = "";
    editing = true;
  }

  async function save(e: SubmitEvent): Promise<void> {
    e.preventDefault();
    if (!draft) return;
    const parsed = fromDraft(draft);
    if (!parsed.ok) {
      error = `Check "${parsed.field}"`;
      return;
    }
    busy = true;
    const res = await engine.send({ type: "set_risk", limits: parsed.limits, confirm: confirmLoosen }, "Risk limits saved");
    busy = false;
    if (res.ok) editing = false;
    else error = res.message;
  }

  async function rearm(): Promise<void> {
    await engine.send({ type: "rearm" }, "Kill switch re-armed. Strategies stay off until you enable them.");
  }

  const used = $derived(r ? Math.min(100, (r.usage.loss_used / Math.max(1, r.limits.daily_max_loss)) * 100) : 0);
</script>

{#if !r}
  <div class="empty">Waiting for risk state...</div>
{:else}
  <div class="grid">
    <section class="panel card">
      <h3>Kill switch</h3>
      {#if r.kill.triggered}
        <p><span class="badge bad">Triggered</span> {label(r.kill.cause ?? "")} at {timeIst(r.kill.at)} IST</p>
        <p class="muted">New entries are blocked. Exits are always allowed.</p>
        <button class="primary" onclick={rearm}>Re-arm</button>
      {:else}
        <p><span class="badge ok">Armed</span> <span class="muted">Hold the KILL button in the top bar to trigger.</span></p>
      {/if}
    </section>

    <section class="panel card">
      <h3>Usage today</h3>
      <div class="meter" style:--u={`${used}%`} class:hot={used >= 80}><span></span></div>
      <dl>
        <dt>Loss used</dt><dd class="num">{rupees(r.usage.loss_used)} of {rupees(r.limits.daily_max_loss)}</dd>
        <dt>P&L</dt><dd class="num">{pnl(r.usage.pnl)}</dd>
        <dt>Trades</dt><dd class="num">{r.usage.trades_today} / {r.limits.max_trades_per_day}</dd>
        <dt>Open positions</dt><dd class="num">{r.usage.open_positions} / {r.limits.max_open_positions}</dd>
        <dt>Orders</dt><dd class="num">{r.usage.orders_last_sec}/s, {r.usage.orders_last_min}/min</dd>
        <dt>Loss streak</dt><dd class="num">{r.usage.loss_streak}</dd>
        {#if r.usage.paused_until}<dt>Paused until</dt><dd class="warn">{timeIst(r.usage.paused_until)} IST</dd>{/if}
      </dl>
    </section>
  </div>

  <section class="panel card limits">
    <div class="head">
      <h3>Limits</h3>
      {#if !editing}<button onclick={edit}>Edit</button>{/if}
    </div>
    {#if editing && draft}
      <form onsubmit={save}>
        <div class="fields">
          {#each FIELDS as f (f.key)}
            <label>
              <span>{f.label}</span>
              {#if f.kind === "bool"}
                <input type="checkbox" checked={draft[f.key] === true} onchange={(e) => draft && (draft[f.key] = e.currentTarget.checked)} />
              {:else}
                <input type="text" value={String(draft[f.key])} oninput={(e) => draft && (draft[f.key] = e.currentTarget.value)} placeholder={f.hint ?? ""} />
              {/if}
            </label>
          {/each}
        </div>
        <label class="confirm">
          <input type="checkbox" bind:checked={confirmLoosen} />
          <span>I am loosening limits during market hours (only allowed with no open positions)</span>
        </label>
        {#if error}<p class="down">{error}</p>{/if}
        <div class="actions">
          <button type="button" onclick={() => (editing = false)}>Cancel</button>
          <button type="submit" class="primary" disabled={busy}>{busy ? "Saving..." : "Save"}</button>
        </div>
      </form>
    {:else}
      <dl class="cols">
        {#each FIELDS as f (f.key)}
          <dt>{f.label}</dt>
          <dd class="num">
            {f.kind === "rupees" ? rupees(Number(r.limits[f.key])) : f.kind === "bool" ? (r.limits[f.key] ? "Yes" : "No") : String(r.limits[f.key])}
          </dd>
        {/each}
      </dl>
    {/if}
  </section>

  <section class="panel card">
    <h3>Recent rejections</h3>
    {#if r.rejections.length === 0}
      <p class="muted">None.</p>
    {:else}
      <table>
        <thead><tr><th>Time</th><th>Strategy</th><th>Token</th><th>Reason</th></tr></thead>
        <tbody>
          {#each r.rejections as j, i (`${j.ts}-${i}`)}
            <tr><td class="mono">{timeIst(j.ts)}</td><td>{j.strategy}</td><td class="mono">{j.token}</td><td class="muted">{j.reason}</td></tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </section>
{/if}

<style>
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(320px, 1fr)); gap: 12px; margin-bottom: 12px; }
  .card { padding: 12px 16px; margin-bottom: 12px; }
  h3 { margin: 0 0 8px; font-size: 13px; color: var(--muted); font-weight: 500; }
  p { margin: 0 0 8px; }
  dl { display: grid; grid-template-columns: 140px 1fr; gap: 6px 8px; margin: 0; }
  dl.cols { grid-template-columns: repeat(3, 200px 120px); }
  dt { color: var(--muted); }
  dd { margin: 0; }
  .meter { height: 6px; background: var(--panel-2); border-radius: 3px; margin-bottom: 10px; overflow: hidden; }
  .meter span { display: block; height: 100%; width: var(--u); background: var(--accent); }
  .meter.hot span { background: var(--down); }
  .head { display: flex; justify-content: space-between; align-items: center; }
  .fields { display: grid; grid-template-columns: repeat(auto-fill, minmax(300px, 1fr)); gap: 8px 16px; }
  .fields label { display: grid; grid-template-columns: 170px 1fr; align-items: center; gap: 8px; }
  .fields input[type="checkbox"] { justify-self: start; }
  .confirm { display: flex; gap: 8px; align-items: center; margin: 12px 0; color: var(--muted); }
  .actions { display: flex; justify-content: flex-end; gap: 8px; }
</style>
