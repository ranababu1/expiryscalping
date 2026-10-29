import { describe, expect, it } from "vitest";
import type { RiskLimitsDto } from "./api";
import { FIELDS, fromDraft, toDraft } from "./riskform";

const limits: RiskLimitsDto = {
  daily_max_loss: 500000,
  per_trade_max_premium: 1500000,
  per_trade_max_lots: 2,
  max_open_positions: 2,
  max_trades_per_day: 40,
  max_orders_per_sec: 5,
  max_orders_per_min: 120,
  entry_start: "09:20",
  entry_end: "14:45",
  flatten_at: "15:10",
  expiry_day_only: true,
  stale_after_ms: 2000,
  max_spread_bps: 300,
  loss_streak_pause: 3,
  loss_pause_minutes: 15,
  margin_check: true,
  limit_buffer_ticks: 2,
  market_protection_pct: 2,
};

describe("risk form", () => {
  it("covers every limit", () => {
    expect(FIELDS.map((f) => f.key).sort()).toEqual(Object.keys(limits).sort());
  });

  it("round trips, money as paise", () => {
    const d = toDraft(limits);
    expect(d.daily_max_loss).toBe("5000.00");
    expect(fromDraft(d)).toEqual({ ok: true, limits });
  });

  it("names the first bad field", () => {
    const d = toDraft(limits);
    d.max_trades_per_day = "4.5";
    expect(fromDraft(d)).toEqual({ ok: false, field: "Max trades per day" });
    const e = toDraft(limits);
    e.flatten_at = "25:00";
    expect(fromDraft(e)).toEqual({ ok: false, field: "Flatten at" });
    const f = toDraft(limits);
    f.daily_max_loss = "-1";
    expect(fromDraft(f)).toEqual({ ok: false, field: "Daily max loss" });
  });
});
