// App-wide engine connection state (Svelte 5 runes).
import { EngineClient, type Command, type ConnState, type Reply } from "./api/client";
import { apply, emptySnapshot, type Snapshot } from "./api/snapshot";
import type { AlertDto } from "./api";

export interface Toast {
  id: number;
  level: AlertDto["level"];
  text: string;
}

class EngineStore {
  conn = $state<ConnState>("idle");
  detail = $state("");
  url = $state("");
  snap = $state.raw<Snapshot>(emptySnapshot());
  toasts = $state<Toast[]>([]);
  private client: EngineClient | null = null;
  private toastId = 1;

  connect(url: string, token: string): void {
    this.client?.close();
    this.url = url;
    this.snap = emptySnapshot();
    this.client = new EngineClient({
      url,
      token,
      onState: (s, d) => {
        this.conn = s;
        this.detail = d;
      },
      onMessage: (m) => {
        this.snap = apply(this.snap, m);
        if (m.type === "alert" && m.level !== "info") this.toast(m.level, m.message);
      },
    });
    this.client.connect();
  }

  disconnect(): void {
    this.client?.close();
    this.client = null;
    this.snap = emptySnapshot();
  }

  /** Sends a command; failures become a toast and are returned. */
  async send(cmd: Command, okText?: string): Promise<Reply> {
    if (!this.client) return { ok: false, code: "disconnected", message: "Not connected" };
    const r = await this.client.request(cmd);
    if (!r.ok) this.toast("error", r.message);
    else if (okText) this.toast("info", r.message ?? okText);
    return r;
  }

  toast(level: Toast["level"], text: string): void {
    const id = this.toastId++;
    this.toasts = [...this.toasts, { id, level, text }].slice(-5);
    setTimeout(() => this.dismiss(id), level === "error" ? 8000 : 4000);
  }

  dismiss(id: number): void {
    this.toasts = this.toasts.filter((t) => t.id !== id);
  }
}

export const engine = new EngineStore();
