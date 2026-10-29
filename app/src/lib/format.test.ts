import { describe, expect, it } from "vitest";
import { dateIst, label, parseRupees, pnl, price, rupees, timeIst } from "./format";

describe("money", () => {
  it("formats paise as rupees with Indian grouping", () => {
    expect(rupees(12345678)).toBe("₹1,23,456.78");
    expect(rupees(5)).toBe("₹0.05");
    expect(rupees(-200000)).toBe("-₹2,000.00");
    expect(rupees(null)).toBe("-");
  });

  it("signs P&L", () => {
    expect(pnl(150)).toBe("+₹1.50");
    expect(pnl(-150)).toBe("-₹1.50");
    expect(pnl(0)).toBe("₹0.00");
  });

  it("prices without a currency sign", () => {
    expect(price(12345)).toBe("123.45");
  });

  it("parses typed rupees into integer paise", () => {
    expect(parseRupees("123")).toBe(12300);
    expect(parseRupees("123.4")).toBe(12340);
    expect(parseRupees(" 1,234.05 ")).toBe(123405);
    expect(parseRupees("0.1")).toBe(10);
  });

  it("rejects bad rupee input", () => {
    for (const bad of ["", "abc", "-5", "1.234", "1e3", "1.", ".5"]) {
      expect(parseRupees(bad)).toBeNull();
    }
  });
});

describe("time", () => {
  it("shows IST", () => {
    // 2026-10-30 03:30:00 UTC is 09:00:00 IST.
    const ms = Date.UTC(2026, 9, 30, 3, 30, 0);
    expect(timeIst(ms)).toBe("09:00:00");
    expect(dateIst(ms)).toBe("2026-10-30");
    expect(timeIst(0)).toBe("-");
  });

  it("labels snake case", () => {
    expect(label("partially_filled")).toBe("Partially filled");
  });
});
