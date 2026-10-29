// WebSocket client for the engine API (engine/crates/es-api). Handles auth,
// request ids, replies and reconnects. Holds no trading logic.
import type { ClientMsg, ErrorCode, ServerMsg } from "./index";

export type ConnState =
  | "idle"
  | "connecting"
  | "authenticating"
  | "connected"
  | "disconnected"
  | "unauthorized";

type DistributiveOmit<T, K extends PropertyKey> = T extends unknown ? Omit<T, K> : never;

/** Any client message except auth, without the request id (added here). */
export type Command = DistributiveOmit<Exclude<ClientMsg, { type: "auth" }>, "req_id">;

export type Reply =
  | { ok: true; message: string | null }
  | { ok: false; code: ErrorCode | "disconnected" | "timeout"; message: string };

export interface SocketLike {
  readyState: number;
  onopen: ((ev: unknown) => void) | null;
  onmessage: ((ev: { data: unknown }) => void) | null;
  onclose: ((ev: unknown) => void) | null;
  onerror: ((ev: unknown) => void) | null;
  send(data: string): void;
  close(): void;
}

export interface ClientOptions {
  url: string;
  token: string;
  onState: (state: ConnState, detail: string) => void;
  onMessage: (msg: ServerMsg) => void;
  /** For tests. Defaults to the browser WebSocket. */
  socket?: (url: string) => SocketLike;
  /** Defaults to 10 s. */
  requestTimeoutMs?: number;
  /** Reconnect delays in ms, the last one repeats. */
  backoffMs?: number[];
}

const OPEN = 1;

interface Pending {
  resolve: (r: Reply) => void;
  timer: ReturnType<typeof setTimeout>;
}

export class EngineClient {
  private ws: SocketLike | null = null;
  private nextId = 1;
  private pending = new Map<number, Pending>();
  private attempt = 0;
  private retry: ReturnType<typeof setTimeout> | null = null;
  private stopped = false;
  private authed = false;
  state: ConnState = "idle";

  constructor(private readonly opts: ClientOptions) {}

  connect(): void {
    this.stopped = false;
    this.open();
  }

  /** Closes for good (no reconnect). */
  close(): void {
    this.stopped = true;
    if (this.retry) clearTimeout(this.retry);
    this.retry = null;
    this.ws?.close();
    this.ws = null;
    this.failPending("disconnected", "closed");
    this.setState("idle", "");
  }

  /** Sends a command and resolves with the engine's ack or error. */
  request(cmd: Command): Promise<Reply> {
    const ws = this.ws;
    if (!ws || ws.readyState !== OPEN || !this.authed) {
      return Promise.resolve({ ok: false, code: "disconnected", message: "Not connected to the engine" });
    }
    const req_id = this.nextId++;
    const msg = { ...cmd, req_id } as ClientMsg;
    return new Promise<Reply>((resolve) => {
      const timer = setTimeout(() => {
        this.pending.delete(req_id);
        resolve({ ok: false, code: "timeout", message: "The engine did not reply in time" });
      }, this.opts.requestTimeoutMs ?? 10_000);
      this.pending.set(req_id, { resolve, timer });
      ws.send(JSON.stringify(msg));
    });
  }

  private open(): void {
    const make = this.opts.socket ?? ((url: string) => new WebSocket(url) as unknown as SocketLike);
    this.authed = false;
    this.setState("connecting", this.opts.url);
    let ws: SocketLike;
    try {
      ws = make(this.opts.url);
    } catch (e) {
      this.setState("disconnected", String(e));
      this.scheduleReconnect();
      return;
    }
    this.ws = ws;
    ws.onopen = () => {
      this.setState("authenticating", "");
      ws.send(JSON.stringify({ type: "auth", token: this.opts.token } satisfies ClientMsg));
    };
    ws.onmessage = (ev) => this.onData(ev.data);
    ws.onerror = () => {
      // onclose follows and handles reconnects.
    };
    ws.onclose = () => {
      if (this.ws !== ws) return;
      this.ws = null;
      this.authed = false;
      this.failPending("disconnected", "Connection lost");
      if (this.stopped || this.state === "unauthorized") return;
      this.setState("disconnected", "Engine not reachable, retrying");
      this.scheduleReconnect();
    };
  }

  private onData(data: unknown): void {
    if (typeof data !== "string") return;
    let msg: ServerMsg;
    try {
      msg = JSON.parse(data) as ServerMsg;
    } catch {
      return;
    }
    switch (msg.type) {
      case "hello":
        this.authed = true;
        this.attempt = 0;
        this.setState("connected", msg.device);
        break;
      case "ack":
        this.settle(msg.req_id, { ok: true, message: msg.message });
        break;
      case "error":
        if (msg.code === "unauthorized" && !this.authed) {
          this.stopped = true;
          this.setState("unauthorized", msg.message);
        } else if (msg.req_id !== null) {
          this.settle(msg.req_id, { ok: false, code: msg.code, message: msg.message });
        }
        break;
      default:
        break;
    }
    this.opts.onMessage(msg);
  }

  private settle(id: number, reply: Reply): void {
    const p = this.pending.get(id);
    if (!p) return;
    clearTimeout(p.timer);
    this.pending.delete(id);
    p.resolve(reply);
  }

  private failPending(code: "disconnected", message: string): void {
    for (const [id, p] of this.pending) {
      clearTimeout(p.timer);
      p.resolve({ ok: false, code, message });
      this.pending.delete(id);
    }
  }

  private scheduleReconnect(): void {
    if (this.stopped || this.retry) return;
    const steps = this.opts.backoffMs ?? [500, 1000, 2000, 5000];
    const delay = steps[Math.min(this.attempt, steps.length - 1)] ?? 5000;
    this.attempt++;
    this.retry = setTimeout(() => {
      this.retry = null;
      if (!this.stopped) this.open();
    }, delay);
  }

  private setState(state: ConnState, detail: string): void {
    this.state = state;
    this.opts.onState(state, detail);
  }
}
