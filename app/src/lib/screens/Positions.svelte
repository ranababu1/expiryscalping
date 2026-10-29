<script lang="ts">
  import Confirm from "../components/Confirm.svelte";
  import { engine } from "../engine.svelte";
  import { pnl, price, rupees } from "../format";

  const ps = $derived(engine.snap.positions);
  const open = $derived(ps?.positions.filter((p) => p.qty !== 0) ?? []);
  const closed = $derived(ps?.positions.filter((p) => p.qty === 0) ?? []);
  let confirmAll = $state(false);

  const cls = (v: number): string => (v > 0 ? "up" : v < 0 ? "down" : "");

  async function exitAll(): Promise<void> {
    confirmAll = false;
    await engine.send({ type: "exit_all" }, "Exiting all positions");
  }
</script>

{#if !ps}
  <div class="empty">Waiting for positions...</div>
{:else}
  <div class="bar">
    <span>Total <strong class="num {cls(ps.pnl.total)}">{pnl(ps.pnl.total)}</strong></span>
    <span class="muted">Realised {pnl(ps.pnl.realised)}</span>
    <span class="muted">Unrealised {pnl(ps.pnl.unrealised)}</span>
    <span class="muted">Charges {rupees(ps.pnl.charges)}</span>
    <span class="muted">Funds {rupees(ps.pnl.available_funds)}</span>
    <button class="danger right" disabled={open.length === 0} onclick={() => (confirmAll = true)}>Exit all</button>
  </div>

  <div class="panel">
    {#if ps.positions.length === 0}
      <div class="empty">No positions today.</div>
    {:else}
      <table>
        <thead>
          <tr>
            <th>Symbol</th><th class="num">Lots</th><th class="num">Qty</th><th class="num">Avg</th>
            <th class="num">LTP</th><th class="num">Unrealised</th><th class="num">Realised</th>
            <th class="num">Charges</th><th></th>
          </tr>
        </thead>
        <tbody>
          {#each [...open, ...closed] as p (p.token)}
            <tr class:closed={p.qty === 0}>
              <td class="mono">{p.symbol}</td>
              <td class="num">{p.lots}</td>
              <td class="num">{p.qty}</td>
              <td class="num">{price(p.avg_price)}</td>
              <td class="num">{price(p.ltp)}</td>
              <td class="num {cls(p.unrealised)}">{pnl(p.unrealised)}</td>
              <td class="num {cls(p.realised)}">{pnl(p.realised)}</td>
              <td class="num muted">{rupees(p.charges)}</td>
              <td class="num">
                {#if p.qty !== 0}
                  <button class="small danger" onclick={() => engine.send({ type: "exit", token: p.token }, "Exit sent")}>Exit</button>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </div>

  {#if engine.snap.fills.length > 0}
    <h3 class="muted">Recent fills</h3>
    <div class="panel">
      <table>
        <thead><tr><th>Order</th><th>Symbol</th><th>Side</th><th class="num">Qty</th><th class="num">Price</th><th>Strategy</th></tr></thead>
        <tbody>
          {#each engine.snap.fills.slice(0, 20) as f, i (`${f.order_id}-${f.ts}-${i}`)}
            <tr>
              <td class="mono">#{f.order_id}</td>
              <td class="mono">{f.symbol}</td>
              <td class={f.side === "buy" ? "up" : "down"}>{f.side.toUpperCase()}</td>
              <td class="num">{f.qty}</td>
              <td class="num">{price(f.price)}</td>
              <td class="muted">{f.strategy}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
{/if}

{#if confirmAll}
  <Confirm
    title="Exit all positions?"
    body="Cancels working entries and closes every open position with exit orders. Strategies keep their current on or off state."
    confirmText="Exit all"
    onconfirm={exitAll}
    oncancel={() => (confirmAll = false)}
  />
{/if}

<style>
  .bar { display: flex; align-items: center; gap: 16px; margin-bottom: 10px; }
  .right { margin-left: auto; }
  tr.closed td { color: var(--muted); }
  h3 { font-size: 13px; font-weight: 500; margin: 16px 0 8px; }
</style>
