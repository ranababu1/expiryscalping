// Display helpers. Money from the engine is always integer paise.

const inr = new Intl.NumberFormat("en-IN", {
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
});

/** 123456 paise -> "₹1,234.56". */
export function rupees(paise: number | null | undefined): string {
  if (paise === null || paise === undefined || !Number.isFinite(paise)) return "-";
  const sign = paise < 0 ? "-" : "";
  return `${sign}₹${inr.format(Math.abs(paise) / 100)}`;
}

/** Same as rupees() but always shows a sign, for P&L. */
export function pnl(paise: number | null | undefined): string {
  if (paise === null || paise === undefined || !Number.isFinite(paise)) return "-";
  return paise > 0 ? `+${rupees(paise)}` : rupees(paise);
}

/** Price without the currency sign: 12345 -> "123.45". */
export function price(paise: number | null | undefined): string {
  if (paise === null || paise === undefined || !Number.isFinite(paise)) return "-";
  return inr.format(paise / 100);
}

/**
 * Parses a rupee amount typed by the user into paise.
 * "123", "123.4", "123.45" are fine; more than two decimals, negatives and
 * junk return null.
 */
export function parseRupees(text: string): number | null {
  const t = text.trim().replace(/,/g, "");
  if (!/^\d+(\.\d{1,2})?$/.test(t)) return null;
  const [whole = "0", frac = ""] = t.split(".");
  const paise = Number(whole) * 100 + Number(frac.padEnd(2, "0"));
  return Number.isSafeInteger(paise) ? paise : null;
}

const istTime = new Intl.DateTimeFormat("en-GB", {
  timeZone: "Asia/Kolkata",
  hour: "2-digit",
  minute: "2-digit",
  second: "2-digit",
  hour12: false,
});

const istDate = new Intl.DateTimeFormat("en-CA", {
  timeZone: "Asia/Kolkata",
  year: "numeric",
  month: "2-digit",
  day: "2-digit",
});

/** Epoch ms -> "14:05:09" IST. */
export function timeIst(ms: number | null | undefined): string {
  if (!ms) return "-";
  return istTime.format(new Date(ms));
}

/** Epoch ms -> "2026-10-30" IST. */
export function dateIst(ms: number | null | undefined): string {
  if (!ms) return "-";
  return istDate.format(new Date(ms));
}

/** Snake case engine names -> "Partially filled". */
export function label(s: string): string {
  const t = s.replace(/_/g, " ");
  return t.charAt(0).toUpperCase() + t.slice(1);
}

export function int(n: number | null | undefined): string {
  if (n === null || n === undefined) return "-";
  return n.toLocaleString("en-IN");
}

export function signed(n: number): string {
  return n > 0 ? `+${int(n)}` : int(n);
}
