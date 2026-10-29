import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import KillSwitch from "./KillSwitch.svelte";

describe("KillSwitch", () => {
  let now = 0;
  let frames: FrameRequestCallback[] = [];
  const tick = (ms: number) => {
    now += ms;
    const f = frames;
    frames = [];
    f.forEach((cb) => cb(now));
  };

  beforeEach(() => {
    now = 0;
    frames = [];
    vi.spyOn(performance, "now").mockImplementation(() => now);
    vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => frames.push(cb));
    vi.stubGlobal("cancelAnimationFrame", () => {
      frames = [];
    });
  });
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("fires only after the full hold", async () => {
    const onfire = vi.fn();
    render(KillSwitch, { triggered: false, holdMs: 1000, onfire });
    const btn = screen.getByRole("button");
    await fireEvent.pointerDown(btn);
    tick(500);
    tick(400);
    expect(onfire).not.toHaveBeenCalled();
    tick(200);
    expect(onfire).toHaveBeenCalledTimes(1);
  });

  it("does not fire when released early", async () => {
    const onfire = vi.fn();
    render(KillSwitch, { triggered: false, holdMs: 1000, onfire });
    const btn = screen.getByRole("button");
    await fireEvent.pointerDown(btn);
    tick(600);
    await fireEvent.pointerUp(btn);
    tick(1000);
    expect(onfire).not.toHaveBeenCalled();
  });

  it("does nothing once triggered", async () => {
    const onfire = vi.fn();
    render(KillSwitch, { triggered: true, holdMs: 1000, onfire });
    const btn = screen.getByRole("button");
    expect(btn.textContent).toContain("KILLED");
    await fireEvent.pointerDown(btn);
    tick(2000);
    expect(onfire).not.toHaveBeenCalled();
  });
});
