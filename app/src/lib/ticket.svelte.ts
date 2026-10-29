// The manual order ticket (place a new buy, or modify a working order).
import type { OrderDto } from "./api";

export type Ticket =
  | { kind: "place"; token: number; symbol: string; lotSize: number; ask: number | null }
  | { kind: "modify"; order: OrderDto; lotSize: number };

class TicketStore {
  open = $state.raw<Ticket | null>(null);
  place(token: number, symbol: string, lotSize: number, ask: number | null): void {
    this.open = { kind: "place", token, symbol, lotSize, ask };
  }
  modify(order: OrderDto, lotSize: number): void {
    this.open = { kind: "modify", order, lotSize };
  }
  close(): void {
    this.open = null;
  }
}

export const ticket = new TicketStore();
