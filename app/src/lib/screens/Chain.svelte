<script lang="ts">
  import type { OptionQuoteDto } from "../api";
  import { engine } from "../engine.svelte";
  import { price, signed } from "../format";
  import { ticket } from "../ticket.svelte";

  const c = $derived(engine.snap.chain);

  function buy(q: OptionQuoteDto | null): void {
    if (!q || !c) return;
    ticket.place(q.token, q.symbol, c.lot_size, q.ask);
  }

  function select(e: Event): void {
    const u = (e.currentTarget as HTMLSelectElement).value;
    void engine.send({ type: "select_chain", underlying: u });
  }

  const iv = (v: number | null): string => (v === null ? "-" : v.toFixed(1));
</script>

{#if !c}
  <div class="empty">Waiting for the option chain...</div>
{:else}
  <div class="bar">
    <select value={c.underlying} onchange={select} aria-label="Underlying">
      {#each c.underlyings as u (u)}<option value={u}>{u}</option>{/each}
    </select>
    <span>Spot <strong class="num">{price(c.spot)}</strong></span>
    <span class="muted">Expiry {c.expiry ?? "-"}</span>
    <span class="muted">Lot {c.lot_size}</span>
    <span class="muted hint">Click an LTP to open a buy ticket.</span>
  </div>

  {#if c.rows.length === 0}
    <div class="empty">No strikes yet for {c.underlying}.</div>
  {:else}
    <div class="panel scroll">
      <table>
        <thead>
          <tr>
            <th class="num">OI chg</th><th class="num">OI</th><th class="num">IV</th>
            <th class="num">Bid</th><th class="num">Ask</th><th class="num">CE LTP</th>
            <th class="strike">Strike</th>
            <th class="num">PE LTP</th><th class="num">Bid</th><th class="num">Ask</th>
            <th class="num">IV</th><th class="num">OI</th><th class="num">OI chg</th>
          </tr>
        </thead>
        <tbody>
          {#each c.rows as r (r.strike)}
            <tr class:atm={r.strike === c.atm_strike}>
              {#if r.ce}
                <td class="num muted">{signed(r.ce.oi_change)}</td>
                <td class="num">{r.ce.oi.toLocaleString("en-IN")}</td>
                <td class="num muted">{iv(r.ce.iv_pct)}</td>
                <td class="num">{price(r.ce.bid)}</td>
                <td class="num">{price(r.ce.ask)}</td>
                <td class="num ltp"><button class="buy" title="Buy {r.ce.symbol}" onclick={() => buy(r.ce)}>{price(r.ce.ltp)}</button></td>
              {:else}
                <td colspan="6" class="muted">-</td>
              {/if}
              <td class="strike mono">{price(r.strike)}</td>
              {#if r.pe}
                <td class="num ltp"><button class="buy" title="Buy {r.pe.symbol}" onclick={() => buy(r.pe)}>{price(r.pe.ltp)}</button></td>
                <td class="num">{price(r.pe.bid)}</td>
                <td class="num">{price(r.pe.ask)}</td>
                <td class="num muted">{iv(r.pe.iv_pct)}</td>
                <td class="num">{r.pe.oi.toLocaleString("en-IN")}</td>
                <td class="num muted">{signed(r.pe.oi_change)}</td>
              {:else}
                <td colspan="6" class="muted">-</td>
              {/if}
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
{/if}

<style>
  .bar { display: flex; align-items: center; gap: 16px; margin-bottom: 10px; }
  .hint { margin-left: auto; font-size: 12px; }
  .scroll { overflow: auto; max-height: calc(100vh - 150px); }
  .strike { text-align: center; background: var(--panel-2); font-weight: 600; }
  th.strike { text-align: center; }
  .ltp { font-weight: 600; }
  .buy { background: none; border: 1px solid transparent; padding: 1px 6px; font-weight: 600; font-variant-numeric: tabular-nums; }
  .buy:hover { border-color: var(--up); color: var(--up); }
  tr.atm td { background: #17212e; }
  tr.atm td.strike { color: var(--accent); }
</style>
