<script lang="ts">
  import { engine } from "../engine.svelte";
  import { label, price, timeIst } from "../format";
  import { ticket } from "../ticket.svelte";

  const orders = $derived(engine.snap.orders?.orders ?? null);
  const working = $derived(orders?.filter((o) => o.working) ?? []);
  const done = $derived(orders?.filter((o) => !o.working) ?? []);
  const lotSize = $derived(engine.snap.chain?.lot_size ?? 1);

  const stateClass = (s: string): string =>
    s === "complete" ? "ok" : s === "rejected" ? "bad" : s === "cancelled" ? "" : "warn";
</script>

{#snippet table(list: NonNullable<typeof orders>, actions: boolean)}
  <table>
    <thead>
      <tr>
        <th>#</th><th>Time</th><th>Symbol</th><th>Side</th><th class="num">Qty</th><th class="num">Filled</th>
        <th class="num">Limit</th><th class="num">Avg</th><th>State</th><th>Strategy</th><th>Reason</th>
        {#if actions}<th></th>{/if}
      </tr>
    </thead>
    <tbody>
      {#each list as o (o.id)}
        <tr>
          <td class="mono">{o.id}</td>
          <td class="mono muted">{timeIst(o.created_at)}</td>
          <td class="mono">{o.symbol}{#if o.is_exit} <span class="badge">exit</span>{/if}</td>
          <td class={o.side === "buy" ? "up" : "down"}>{o.side.toUpperCase()}</td>
          <td class="num">{o.qty}</td>
          <td class="num">{o.filled}</td>
          <td class="num">{o.order_type === "market" ? "MKT" : price(o.limit)}</td>
          <td class="num">{o.filled > 0 ? price(o.avg_fill_price) : "-"}</td>
          <td><span class="badge {stateClass(o.state)}" title={o.message ?? ""}>{label(o.state)}</span></td>
          <td class="muted">{o.strategy}</td>
          <td class="muted reason" title={o.message ?? o.reason}>{o.reason}</td>
          {#if actions}
            <td class="num actions">
              <button class="small" disabled={o.is_exit && o.order_type === "market"} onclick={() => ticket.modify(o, lotSize)}>Modify</button>
              <button class="small danger" onclick={() => engine.send({ type: "cancel", order_id: o.id }, "Cancel sent")}>Cancel</button>
            </td>
          {/if}
        </tr>
      {/each}
    </tbody>
  </table>
{/snippet}

{#if !orders}
  <div class="empty">Waiting for orders...</div>
{:else}
  <h3>Working <span class="muted">{working.length}</span></h3>
  <div class="panel">
    {#if working.length === 0}<div class="empty">No working orders.</div>{:else}{@render table(working, true)}{/if}
  </div>
  <h3>Today <span class="muted">{done.length}</span></h3>
  <div class="panel scroll">
    {#if done.length === 0}<div class="empty">No orders yet today.</div>{:else}{@render table(done, false)}{/if}
  </div>
{/if}

<style>
  h3 { font-size: 13px; font-weight: 500; margin: 0 0 8px; }
  h3:not(:first-child) { margin-top: 16px; }
  .scroll { overflow: auto; max-height: calc(100vh - 330px); }
  .reason { max-width: 220px; overflow: hidden; text-overflow: ellipsis; }
  .actions { display: flex; gap: 6px; justify-content: flex-end; }
</style>
