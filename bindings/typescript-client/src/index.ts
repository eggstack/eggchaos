/** Native control client for eggchaos (`@eggstack/eggchaos-client`). */

export { EggchaosClient } from "./client.js";
export type { EggchaosClientOptions, RequestOptions } from "./client.js";
export { EggchaosContractError, EggchaosError, EggchaosTransportError } from "./errors.js";
export {
  decodeDatagramFault,
  decodeStreamFault,
} from "./models.js";
export type {
  BandwidthFault,
  BlackholeFault,
  DatagramBandwidthFault,
  DatagramCorruptFault,
  DatagramDelayFault,
  DatagramDuplicateFault,
  DatagramFaultKind,
  DatagramFaultPatchWire,
  DatagramFaultSpecWire,
  DatagramLossFault,
  DatagramProxyCreate,
  DatagramProxyPatch,
  DatagramReorderFault,
  Direction,
  DisconnectFault,
  FaultPatchWire,
  LatencyFault,
  LimitDataFault,
  ProxyCreate,
  ProxyPatch,
  ScenarioActionWire,
  ScenarioV1Wire,
  ScheduleV2Wire,
  SliceFault,
  SlowCloseFault,
  StreamFaultKind,
} from "./models.js";
export {
  DATAGRAM_FAULT_TAGS,
  OPENAPI_VERSION,
  OPERATIONS,
  SCENARIO_ACTION_TAGS,
  STREAM_FAULT_TAGS,
} from "./generated.js";
