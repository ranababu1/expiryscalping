import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { EngineClient, type ConnState, type SocketLike } from "./client";
import type { ServerMsg } from "./index";

class FakeSocket implements SocketLike {
  readyState = 0;
  sent: string[] = [];
  onopen: ((ev: unknown) => void) | null = null;
  onmessage: ((ev: { data: unknown }) => void) | null = null;
  onclose: ((ev: unknown) => void) | null = null;
  onerror: ((ev: unknown) => void) | null = null;
  send(data: string): void {
    this.sent.push(data);
  }
  close(): void {
    this.readyState = 3;
    this.onclose?.({});
  }
  open(): void {
    this.readyState = 1;
    this.onopen?.({});
  }
  push(msg: ServerMsg): void {
    this.onmessage?.({ data: JSON.stringify(msg) });
  }
  drop(): void {
    this.readyState = 3;
    this.onclose?.({});
  }
}

function setup() {
  const sockets: FakeSocket[] = [];
  const states: ConnState[] = [];
  const messages: ServerMsg[] = [];
  const client = new EngineClient({
    url: "ws://127.0.0.1:7400",
    token: "secret",
    onState: (s) => states.push(s),
    onMessage: (m) => messages.push(m),
    socket: () => {
      const s = new FakeSocket();
      sockets.push(s);
      return s;
    },
    requestTimeoutMs: 1000,
    backoffMs: [100, 200],
  });
  return { client, sockets, states, messages };
}

const hello: ServerMsg = { type: "hello", protocol: 1, engine_version: "0.2.0", device: "desktop" };

describe("EngineClient", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("authenticates first, then reports connected on hello", () => {
    const { client, sockets, states } = setup();
    client.connect();
    const ws = sockets[0]!;
    ws.open();
    expect(JSON.parse(ws.sent[0]!)).toEqual({ type: "auth", token: "secret" });
    ws.push(hello);
    expect(states).toEqual(["connecting", "authenticating", "connected"]);
  });

  it("matches replies to requests by req_id", async () => {
    const { client, sockets } = setup();
    client.connect();
    const ws = sockets[0]!;
    ws.open();
    ws.push(hello);
    const a = client.request({ type: "kill" });
    const b = client.request({ type: "exit_all" });
    const sentA = JSON.parse(ws.sent[1]!) as { req_id: number; type: string };
    const sentB = JSON.parse(ws.sent[2]!) as { req_id: number; type: string };
    expect(sentA.type).toBe("kill");
    expect(sentA.req_id).not.toBe(sentB.req_id);
    ws.push({ type: "error", req_id: sentB.req_id, code: "not_allowed", message: "no" });
    ws.push({ type: "ack", req_id: sentA.req_id, message: null });
    await expect(a).resolves.toEqual({ ok: true, message: null });
    await expect(b).resolves.toEqual({ ok: false, code: "not_allowed", message: "no" });
  });

  it("refuses requests while not connected", async () => {
    const { client } = setup();
    await expect(client.request({ type: "kill" })).resolves.toMatchObject({ ok: false, code: "disconnected" });
  });

  it("times out a request without a reply", async () => {
    const { client, sockets } = setup();
    client.connect();
    sockets[0]!.open();
    sockets[0]!.push(hello);
    const p = client.request({ type: "ping" });
    vi.advanceTimersByTime(1001);
    await expect(p).resolves.toMatchObject({ ok: false, code: "timeout" });
  });

  it("reconnects with backoff and fails pending requests on drop", async () => {
    const { client, sockets, states } = setup();
    client.connect();
    sockets[0]!.open();
    sockets[0]!.push(hello);
    const p = client.request({ type: "ping" });
    sockets[0]!.drop();
    await expect(p).resolves.toMatchObject({ ok: false, code: "disconnected" });
    expect(states.at(-1)).toBe("disconnected");
    vi.advanceTimersByTime(99);
    expect(sockets).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(sockets).toHaveLength(2);
    sockets[1]!.drop();
    vi.advanceTimersByTime(199);
    expect(sockets).toHaveLength(2);
    vi.advanceTimersByTime(1);
    expect(sockets).toHaveLength(3);
  });

  it("stops on a bad token", () => {
    const { client, sockets, states } = setup();
    client.connect();
    sockets[0]!.open();
    sockets[0]!.push({ type: "error", req_id: null, code: "unauthorized", message: "bad token" });
    sockets[0]!.drop();
    vi.advanceTimersByTime(10_000);
    expect(sockets).toHaveLength(1);
    expect(states.at(-1)).toBe("unauthorized");
  });

  it("does not reconnect after close()", () => {
    const { client, sockets, states } = setup();
    client.connect();
    sockets[0]!.open();
    client.close();
    vi.advanceTimersByTime(10_000);
    expect(sockets).toHaveLength(1);
    expect(states.at(-1)).toBe("idle");
  });
});
