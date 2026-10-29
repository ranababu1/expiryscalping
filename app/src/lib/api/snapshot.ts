// Latest state per stream, as pushed by the engine. Pure functions only:
// the app displays what the engine computed and never derives trading
// decisions itself.
import type {
  AlertDto,
  ChainDto,
  FillDto,
  OrdersDto,
  PositionsDto,
  RiskDto,
  ServerMsg,
  StatusDto,
  StrategiesDto,
} from "./index";

export const MAX_ALERTS = 50;
export const MAX_FILLS = 200;

export interface Hello {
  protocol: number;
  engine_version: string;
  device: string;
}

export interface Snapshot {
  hello: Hello | null;
  status: StatusDto | null;
  chain: ChainDto | null;
  positions: PositionsDto | null;
  orders: OrdersDto | null;
  risk: RiskDto | null;
  strategies: StrategiesDto | null;
  /** Newest first. */
  alerts: AlertDto[];
  /** Newest first. */
  fills: FillDto[];
}

export function emptySnapshot(): Snapshot {
  return {
    hello: null,
    status: null,
    chain: null,
    positions: null,
    orders: null,
    risk: null,
    strategies: null,
    alerts: [],
    fills: [],
  };
}

function strip<T extends { type: string }>(msg: T): Omit<T, "type"> {
  const { type: _type, ...rest } = msg;
  return rest;
}

/** Returns a new snapshot with `msg` applied. Ack and error are not state. */
export function apply(snap: Snapshot, msg: ServerMsg): Snapshot {
  switch (msg.type) {
    case "hello":
      return { ...snap, hello: strip(msg) };
    case "status":
      return { ...snap, status: strip(msg) as StatusDto };
    case "chain":
      return { ...snap, chain: strip(msg) as ChainDto };
    case "positions":
      return { ...snap, positions: strip(msg) as PositionsDto };
    case "orders":
      return { ...snap, orders: strip(msg) as OrdersDto };
    case "risk":
      return { ...snap, risk: strip(msg) as RiskDto };
    case "strategies":
      return { ...snap, strategies: strip(msg) as StrategiesDto };
    case "alert":
      return { ...snap, alerts: [strip(msg) as AlertDto, ...snap.alerts].slice(0, MAX_ALERTS) };
    case "fill":
      return { ...snap, fills: [strip(msg) as FillDto, ...snap.fills].slice(0, MAX_FILLS) };
    case "ack":
    case "error":
      return snap;
  }
}
