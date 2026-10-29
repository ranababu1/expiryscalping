// Field metadata for the risk limits form. Money fields are typed in rupees
// and sent as integer paise. Validation of the values themselves is the
// engine's job; this only checks that the input parses.
import type { RiskLimitsDto } from "./api";
import { parseRupees } from "./format";

export type FieldKind = "rupees" | "int" | "time" | "bool";

export interface Field {
  key: keyof RiskLimitsDto;
  label: string;
  kind: FieldKind;
  hint?: string;
}

export const FIELDS: Field[] = [
  { key: "daily_max_loss", label: "Daily max loss", kind: "rupees", hint: "Kill switch fires at this loss" },
  { key: "per_trade_max_premium", label: "Max premium per trade", kind: "rupees" },
  { key: "per_trade_max_lots", label: "Max lots per trade", kind: "int" },
  { key: "max_open_positions", label: "Max open positions", kind: "int" },
  { key: "max_trades_per_day", label: "Max trades per day", kind: "int" },
  { key: "max_orders_per_sec", label: "Max orders per second", kind: "int" },
  { key: "max_orders_per_min", label: "Max orders per minute", kind: "int" },
  { key: "entry_start", label: "Entries from", kind: "time", hint: "HH:MM IST" },
  { key: "entry_end", label: "Entries until", kind: "time", hint: "HH:MM IST" },
  { key: "flatten_at", label: "Flatten at", kind: "time", hint: "HH:MM IST" },
  { key: "expiry_day_only", label: "Expiry day only", kind: "bool" },
  { key: "stale_after_ms", label: "Stale data after (ms)", kind: "int" },
  { key: "max_spread_bps", label: "Max spread (bps)", kind: "int" },
  { key: "loss_streak_pause", label: "Pause after losses in a row", kind: "int" },
  { key: "loss_pause_minutes", label: "Pause length (min)", kind: "int" },
  { key: "margin_check", label: "Margin check", kind: "bool" },
  { key: "limit_buffer_ticks", label: "Limit buffer (ticks)", kind: "int" },
  { key: "market_protection_pct", label: "Market protection (%)", kind: "int" },
];

export type Draft = Record<keyof RiskLimitsDto, string | boolean>;

export function toDraft(l: RiskLimitsDto): Draft {
  const d = {} as Draft;
  for (const f of FIELDS) {
    const v = l[f.key];
    if (f.kind === "bool") d[f.key] = Boolean(v);
    else if (f.kind === "rupees") d[f.key] = (Number(v) / 100).toFixed(2);
    else d[f.key] = String(v);
  }
  return d;
}

/** Returns the limits, or the label of the first field that does not parse. */
export function fromDraft(d: Draft): { ok: true; limits: RiskLimitsDto } | { ok: false; field: string } {
  const out: Record<string, unknown> = {};
  for (const f of FIELDS) {
    const v = d[f.key];
    if (f.kind === "bool") {
      out[f.key] = v === true;
      continue;
    }
    const s = String(v).trim();
    if (f.kind === "rupees") {
      const p = parseRupees(s);
      if (p === null) return { ok: false, field: f.label };
      out[f.key] = p;
    } else if (f.kind === "int") {
      if (!/^\d+$/.test(s) || !Number.isSafeInteger(Number(s))) return { ok: false, field: f.label };
      out[f.key] = Number(s);
    } else {
      if (!/^([01]\d|2[0-3]):[0-5]\d$/.test(s)) return { ok: false, field: f.label };
      out[f.key] = s;
    }
  }
  return { ok: true, limits: out as RiskLimitsDto };
}
