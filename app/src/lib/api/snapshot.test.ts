import { describe, expect, it } from "vitest";
import { apply, emptySnapshot, MAX_ALERTS } from "./snapshot";
import type { ServerMsg } from "./index";

describe("snapshot", () => {
  it("keeps the latest state per stream without the type tag", () => {
    const msg: ServerMsg = {
      type: "strategies",
      ai_paused: true,
      strategies: [{ id: "momentum", enabled: false, status: "idle", description: "d" }],
    };
    const s = apply(emptySnapshot(), msg);
    expect(s.strategies).toEqual({
      ai_paused: true,
      strategies: [{ id: "momentum", enabled: false, status: "idle", description: "d" }],
    });
  });

  it("caps alerts newest first", () => {
    let s = emptySnapshot();
    for (let i = 0; i < MAX_ALERTS + 5; i++) {
      s = apply(s, { type: "alert", level: "info", code: "X", message: String(i), ts: i });
    }
    expect(s.alerts).toHaveLength(MAX_ALERTS);
    expect(s.alerts[0]?.message).toBe(String(MAX_ALERTS + 4));
  });

  it("ignores acks", () => {
    const s = emptySnapshot();
    expect(apply(s, { type: "ack", req_id: 1, message: null })).toBe(s);
  });
});
