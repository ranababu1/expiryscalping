<script lang="ts">
  import { engine } from "../engine.svelte";
  import { price, parseRupees, rupees } from "../format";
  import { ticket, type Ticket } from "../ticket.svelte";

  interface Props {
    t: Ticket;
  }
  let { t }: Props = $props();

  const initialLots = t.kind === "modify" ? Math.max(1, Math.round(t.order.qty / Math.max(1, t.lotSize))) : 1;
  const initialLimit =
    t.kind === "modify" ? (t.order.limit !== null ? (t.order.limit / 100).toFixed(2) : "") : t.ask !== null ? (t.ask / 100).toFixed(2) : "";

  let lots = $state(initialLots);
  let limitText = $state(initialLimit);
  let busy = $state(false);
  let error = $state("");

  const symbol = $derived(t.kind === "modify" ? t.order.symbol : t.symbol);
  const limit = $derived(limitText.trim() === "" ? null : parseRupees(limitText));
  const limitBad = $derived(limitText.trim() !== "" && limit === null);
  const lotsBad = $derived(!Number.isInteger(lots) || lots < 1);
  const qty = $derived(lotsBad ? 0 : lots * t.lotSize);
  const estimate = $derived(limit !== null ? limit * qty : t.kind === "place" && t.ask !== null ? t.ask * qty : null);

  async function submit(e: SubmitEvent): Promise<void> {
    e.preventDefault();
    if (lotsBad || limitBad || busy) return;
    if (t.kind === "modify" && limit === null) {
      error = "A modify needs a limit price";
      return;
    }
    busy = true;
    error = "";
    const r =
      t.kind === "place"
        ? await engine.send({ type: "place", token: t.token, lots, limit }, "Order sent")
        : await engine.send({ type: "modify", order_id: t.order.id, lots, limit: limit ?? 0 }, "Modify sent");
    busy = false;
    if (r.ok) ticket.close();
    else error = r.message;
  }

  function keydown(e: KeyboardEvent): void {
    if (e.key === "Escape") ticket.close();
  }
</script>

<svelte:window onkeydown={keydown} />

<div class="backdrop" role="presentation" onclick={() => ticket.close()}></div>
<form class="ticket panel" onsubmit={submit} aria-label="Order ticket">
  <header>
    <strong>{t.kind === "place" ? "Buy" : `Modify #${t.order.id}`}</strong>
    <span class="mono">{symbol}</span>
  </header>

  <label>
    <span>Lots</span>
    <input type="number" min="1" step="1" bind:value={lots} aria-invalid={lotsBad} />
    <span class="muted">{qty} qty (lot {t.lotSize})</span>
  </label>

  <label>
    <span>Limit (₹)</span>
    <input
      type="text"
      inputmode="decimal"
      placeholder={t.kind === "place" ? "blank = marketable limit" : ""}
      bind:value={limitText}
      aria-invalid={limitBad}
    />
    {#if t.kind === "place" && t.ask !== null}
      <span class="muted">ask {price(t.ask)}</span>
    {/if}
  </label>

  <div class="row muted">
    <span>Premium approx.</span>
    <span class="num">{rupees(estimate)}</span>
  </div>
  <p class="muted note">Every order goes through the engine's risk checks. Paper mode.</p>

  {#if error}<p class="down">{error}</p>{/if}

  <footer>
    <button type="button" onclick={() => ticket.close()}>Cancel</button>
    <button type="submit" class="primary" disabled={busy || lotsBad || limitBad}>
      {busy ? "Sending..." : t.kind === "place" ? "Place buy" : "Modify"}
    </button>
  </footer>
</form>

<style>
  .backdrop { position: fixed; inset: 0; background: rgb(0 0 0 / 0.5); z-index: 20; }
  .ticket {
    position: fixed;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: 360px;
    padding: 16px;
    z-index: 21;
    display: grid;
    gap: 12px;
  }
  header { display: flex; justify-content: space-between; gap: 8px; }
  label { display: grid; grid-template-columns: 80px 1fr; gap: 4px 8px; align-items: center; }
  label .muted { grid-column: 2; font-size: 12px; }
  .row { display: flex; justify-content: space-between; }
  .note { margin: 0; font-size: 12px; }
  p { margin: 0; }
  footer { display: flex; justify-content: flex-end; gap: 8px; }
</style>
