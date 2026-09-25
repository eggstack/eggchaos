/** Typed native models. Optional fields use `undefined` for absence so
 * server defaults apply (`JSON.stringify` drops `undefined`). */

import { EggchaosContractError } from "./errors.js";

export type Direction = "upstream" | "downstream";

export interface LatencyFault {
  type: "latency";
  delay_ns: number;
  jitter_ns?: number;
  max_buffer_bytes?: number;
}
export interface BandwidthFault {
  type: "bandwidth";
  bytes_per_second?: number;
  burst_bytes?: number;
}
export interface BlackholeFault {
  type: "blackhole";
  close_after_ns?: number | null;
}
export interface LimitDataFault {
  type: "limit-data";
  bytes?: number;
}
export interface SlowCloseFault {
  type: "slow-close";
  delay_ns: number;
}
export interface SliceFault {
  type: "slice";
  average_size?: number;
  variation?: number;
  delay_ns?: number;
}
export interface DisconnectFault {
  type: "disconnect";
  after_ns?: number;
  hard_reset?: boolean;
}

export type StreamFaultKind =
  | LatencyFault
  | BandwidthFault
  | BlackholeFault
  | LimitDataFault
  | SlowCloseFault
  | SliceFault
  | DisconnectFault;

const STREAM_FAULT_TYPES: ReadonlySet<string> = new Set([
  "latency",
  "bandwidth",
  "blackhole",
  "limit-data",
  "slow-close",
  "slice",
  "disconnect",
]);

export function decodeStreamFault(data: unknown): StreamFaultKind {
  const tag = (data as { type?: unknown })?.type;
  if (typeof tag !== "string" || !STREAM_FAULT_TYPES.has(tag)) {
    throw new EggchaosContractError(`unknown stream fault type: ${String(tag)}`);
  }
  return data as StreamFaultKind;
}

export interface DatagramDelayFault {
  type: "delay";
  delay_ns: number;
  jitter_ns?: number;
}
export interface DatagramLossFault {
  type: "loss";
}
export interface DatagramDuplicateFault {
  type: "duplicate";
  additional_copies: number;
}
export interface DatagramReorderFault {
  type: "reorder";
  hold_ns: number;
}
export interface DatagramCorruptFault {
  type: "payload-corrupt";
  bytes: number;
}
export interface DatagramBandwidthFault {
  type: "bandwidth";
  bytes_per_second: number;
  burst_bytes: number;
}

export type DatagramFaultKind =
  | DatagramDelayFault
  | DatagramLossFault
  | DatagramDuplicateFault
  | DatagramReorderFault
  | DatagramCorruptFault
  | DatagramBandwidthFault;

const DATAGRAM_FAULT_TYPES: ReadonlySet<string> = new Set([
  "delay",
  "loss",
  "duplicate",
  "reorder",
  "payload-corrupt",
  "bandwidth",
]);

export function decodeDatagramFault(data: unknown): DatagramFaultKind {
  const tag = (data as { type?: unknown })?.type;
  if (typeof tag !== "string" || !DATAGRAM_FAULT_TYPES.has(tag)) {
    throw new EggchaosContractError(`unknown datagram fault type: ${String(tag)}`);
  }
  return data as DatagramFaultKind;
}

export interface ProxyCreate {
  name: string;
  listen: string;
  upstream: string;
  enabled?: boolean;
  max_connections?: number;
  connect_timeout_ms?: number;
  seed?: number;
}

export interface ProxyPatch {
  listen?: string;
  upstream?: string;
  enabled?: boolean;
  max_connections?: number;
  connect_timeout_ms?: number;
}

export interface DatagramFaultSpecWire {
  id: string;
  probability?: number;
  kind: DatagramFaultKind;
}

export interface DatagramProxyCreate {
  name: string;
  listen: string;
  upstream: string;
  max_associations?: number;
  association_idle_timeout_ms?: number;
  max_queued_datagrams?: number;
  max_queued_bytes?: number;
  max_datagram_size?: number;
  seed?: number;
  upstream_faults?: DatagramFaultSpecWire[];
  downstream_faults?: DatagramFaultSpecWire[];
}

export interface DatagramProxyPatch {
  enabled?: boolean;
  listen?: string;
  upstream?: string;
  max_associations?: number;
  association_idle_timeout_ms?: number;
}

export interface FaultPatchWire {
  probability?: number;
  kind?: StreamFaultKind;
}

export interface DatagramFaultPatchWire {
  probability?: number;
  kind?: DatagramFaultKind;
}

/** A scenario action in native wire form (V1 and V2 share the tags). */
export interface ScenarioActionWire {
  type: "set-plan" | "remove-fault" | "set-datagram-plan" | "remove-datagram-fault";
  proxy: string;
  direction: Direction;
  faults?: unknown[];
  id?: string;
}

export interface ScenarioV1Wire {
  version: 1;
  seed: number;
  events: Array<{ at_ms: number; action: ScenarioActionWire }>;
}

export interface ScheduleV2Wire {
  version: 2;
  seed: number;
  execution_key: number;
  isolation?: "strict" | "live";
  cleanup?: "restore-initial" | "leave";
  phases?: Array<{ name?: string; duration_ns: number; actions: ScenarioActionWire[] }>;
  repeat?: unknown;
}
