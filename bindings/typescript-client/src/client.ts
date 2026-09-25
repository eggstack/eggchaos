/** Promise-based native control client over `fetch` (no runtime dependencies). */

import { EggchaosError, EggchaosTransportError } from "./errors.js";
import {
  DatagramFaultKind,
  DatagramFaultPatchWire,
  DatagramProxyCreate,
  DatagramProxyPatch,
  Direction,
  FaultPatchWire,
  ProxyCreate,
  ProxyPatch,
  ScheduleV2Wire,
  ScenarioV1Wire,
  StreamFaultKind,
} from "./models.js";

export interface EggchaosClientOptions {
  baseUrl?: string;
  token?: string;
  timeoutMs?: number;
  /** Injectable transport for tests; defaults to the global `fetch`. */
  fetch?: typeof fetch;
}

export interface RequestOptions {
  /** Per-request cancellation. */
  signal?: AbortSignal;
}

type Json = unknown;

export class EggchaosClient {
  private readonly baseUrl: string;
  private readonly token?: string;
  private readonly timeoutMs: number;
  private readonly fetchImpl: typeof fetch;

  constructor(options: EggchaosClientOptions = {}) {
    const baseUrl = (options.baseUrl ?? "http://127.0.0.1:8475").replace(/\/+$/, "");
    let parsed: URL;
    try {
      parsed = new URL(baseUrl);
    } catch {
      throw new Error("baseUrl must be an absolute http(s) URL");
    }
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
      throw new Error("baseUrl must use http or https");
    }
    if (parsed.username || parsed.password) {
      throw new Error("baseUrl must not embed userinfo; pass token= instead");
    }
    this.baseUrl = baseUrl;
    this.token = options.token;
    this.timeoutMs = options.timeoutMs ?? 10_000;
    this.fetchImpl = options.fetch ?? globalThis.fetch.bind(globalThis);
  }

  private async request(
    method: string,
    path: string,
    body?: unknown,
    options: RequestOptions = {},
  ): Promise<Json> {
    const headers: Record<string, string> = { Accept: "application/json" };
    if (this.token !== undefined) {
      headers["Authorization"] = `Bearer ${this.token}`;
    }
    const init: RequestInit = { method, headers, signal: options.signal };
    if (body !== undefined) {
      headers["Content-Type"] = "application/json";
      init.body = JSON.stringify(body);
    }
    let timeout: ReturnType<typeof setTimeout> | undefined;
    let signal = options.signal;
    if (this.timeoutMs > 0 && typeof AbortSignal.timeout === "function" && !signal) {
      signal = AbortSignal.timeout(this.timeoutMs);
    } else if (this.timeoutMs > 0 && !signal) {
      const controller = new AbortController();
      timeout = setTimeout(() => controller.abort(), this.timeoutMs);
      signal = controller.signal;
    }
    init.signal = signal ?? undefined;
    let response: Response;
    try {
      response = await this.fetchImpl(this.baseUrl + path, init);
    } catch (error) {
      if (error instanceof EggchaosError) throw error;
      throw new EggchaosTransportError(
        error instanceof Error ? error.message : String(error),
        { cause: error },
      );
    } finally {
      if (timeout !== undefined) clearTimeout(timeout);
    }
    const text = await response.text();
    if (response.status === 200 || response.status === 201 || response.status === 202) {
      if (!text) return null;
      try {
        return JSON.parse(text) as Json;
      } catch (error) {
        throw new EggchaosTransportError(`invalid response encoding: ${String(error)}`);
      }
    }
    if (!text) {
      throw new EggchaosError(response.status, "empty", "empty error response");
    }
    try {
      const envelope = JSON.parse(text) as {
        error?: { code?: unknown; message?: unknown; detail?: unknown };
      };
      const code = String(envelope?.error?.code ?? "unknown");
      const detail = String(
        envelope?.error?.detail ?? envelope?.error?.message ?? text,
      );
      throw new EggchaosError(response.status, code, detail);
    } catch (error) {
      if (error instanceof EggchaosError) throw error;
      throw new EggchaosError(response.status, "unparseable", text.slice(0, 512));
    }
  }

  private async requestText(path: string, options: RequestOptions = {}): Promise<string> {
    // Metrics are Prometheus text, never JSON.
    const headers: Record<string, string> = {};
    if (this.token !== undefined) {
      headers["Authorization"] = `Bearer ${this.token}`;
    }
    let response: Response;
    try {
      response = await this.fetchImpl(this.baseUrl + path, {
        headers,
        signal: options.signal,
      });
    } catch (error) {
      throw new EggchaosTransportError(
        error instanceof Error ? error.message : String(error),
        { cause: error },
      );
    }
    if (response.status !== 200) {
      throw new EggchaosError(response.status, "metrics", "metrics request failed");
    }
    return response.text();
  }

  private static quote(value: string): string {
    return encodeURIComponent(value);
  }

  health(options?: RequestOptions): Promise<Json> {
    return this.request("GET", "/v1/health", undefined, options);
  }

  version(options?: RequestOptions): Promise<Json> {
    return this.request("GET", "/v1/version", undefined, options);
  }

  metricsText(options?: RequestOptions): Promise<string> {
    return this.requestText("/metrics", options);
  }

  reset(options?: RequestOptions): Promise<Json> {
    return this.request("POST", "/v1/reset", undefined, options);
  }

  listProxies(options?: RequestOptions): Promise<Json> {
    return this.request("GET", "/v1/proxies", undefined, options);
  }

  createProxy(proxy: ProxyCreate, options?: RequestOptions): Promise<Json> {
    return this.request("POST", "/v1/proxies", proxy, options);
  }

  getProxy(name: string, options?: RequestOptions): Promise<Json> {
    return this.request("GET", `/v1/proxies/${EggchaosClient.quote(name)}`, undefined, options);
  }

  patchProxy(name: string, patch: ProxyPatch, options?: RequestOptions): Promise<Json> {
    return this.request("PATCH", `/v1/proxies/${EggchaosClient.quote(name)}`, patch, options);
  }

  deleteProxy(name: string, options?: RequestOptions): Promise<Json> {
    return this.request(
      "DELETE",
      `/v1/proxies/${EggchaosClient.quote(name)}`,
      undefined,
      options,
    );
  }

  listFaults(proxy: string, options?: RequestOptions): Promise<Json> {
    return this.request(
      "GET",
      `/v1/proxies/${EggchaosClient.quote(proxy)}/faults`,
      undefined,
      options,
    );
  }

  addFault(
    proxy: string,
    direction: Direction,
    id: string,
    kind: StreamFaultKind,
    probability?: number,
    options?: RequestOptions,
  ): Promise<Json> {
    const body: Record<string, unknown> = { direction, id, kind };
    if (probability !== undefined) body["probability"] = probability;
    return this.request(
      "POST",
      `/v1/proxies/${EggchaosClient.quote(proxy)}/faults`,
      body,
      options,
    );
  }

  getFault(proxy: string, id: string, options?: RequestOptions): Promise<Json> {
    return this.request(
      "GET",
      `/v1/proxies/${EggchaosClient.quote(proxy)}/faults/${EggchaosClient.quote(id)}`,
      undefined,
      options,
    );
  }

  patchFault(
    proxy: string,
    id: string,
    patch: FaultPatchWire,
    options?: RequestOptions,
  ): Promise<Json> {
    return this.request(
      "PATCH",
      `/v1/proxies/${EggchaosClient.quote(proxy)}/faults/${EggchaosClient.quote(id)}`,
      patch,
      options,
    );
  }

  deleteFault(proxy: string, id: string, options?: RequestOptions): Promise<Json> {
    return this.request(
      "DELETE",
      `/v1/proxies/${EggchaosClient.quote(proxy)}/faults/${EggchaosClient.quote(id)}`,
      undefined,
      options,
    );
  }

  listConnections(options?: RequestOptions): Promise<Json> {
    return this.request("GET", "/v1/connections", undefined, options);
  }

  getConnection(id: number, options?: RequestOptions): Promise<Json> {
    return this.request("GET", `/v1/connections/${Math.trunc(id)}`, undefined, options);
  }

  killConnection(id: number, options?: RequestOptions): Promise<Json> {
    return this.request("DELETE", `/v1/connections/${Math.trunc(id)}`, undefined, options);
  }

  history(options?: RequestOptions): Promise<Json> {
    return this.request("GET", "/v1/history", undefined, options);
  }

  applyScenario(
    scenario: ScenarioV1Wire | ScheduleV2Wire | unknown,
    options?: RequestOptions,
  ): Promise<Json> {
    return this.request("POST", "/v1/scenarios/apply", scenario, options);
  }

  validateSchedule(
    schedule: ScheduleV2Wire | unknown,
    options?: RequestOptions,
  ): Promise<Json> {
    return this.request("POST", "/v1/scenarios/validate", schedule, options);
  }

  compileSchedule(
    schedule: ScheduleV2Wire | unknown,
    options?: RequestOptions,
  ): Promise<Json> {
    return this.request("POST", "/v1/scenarios/compile", schedule, options);
  }

  getScenario(runId: number, options?: RequestOptions): Promise<Json> {
    return this.request("GET", `/v1/scenarios/${Math.trunc(runId)}`, undefined, options);
  }

  cancelScenario(runId: number, options?: RequestOptions): Promise<Json> {
    return this.request("DELETE", `/v1/scenarios/${Math.trunc(runId)}`, undefined, options);
  }

  listDatagramProxies(options?: RequestOptions): Promise<Json> {
    return this.request("GET", "/v1/datagram-proxies", undefined, options);
  }

  createDatagramProxy(proxy: DatagramProxyCreate, options?: RequestOptions): Promise<Json> {
    return this.request("POST", "/v1/datagram-proxies", proxy, options);
  }

  getDatagramProxy(name: string, options?: RequestOptions): Promise<Json> {
    return this.request(
      "GET",
      `/v1/datagram-proxies/${EggchaosClient.quote(name)}`,
      undefined,
      options,
    );
  }

  patchDatagramProxy(
    name: string,
    patch: DatagramProxyPatch,
    options?: RequestOptions,
  ): Promise<Json> {
    return this.request(
      "PATCH",
      `/v1/datagram-proxies/${EggchaosClient.quote(name)}`,
      patch,
      options,
    );
  }

  deleteDatagramProxy(name: string, options?: RequestOptions): Promise<Json> {
    return this.request(
      "DELETE",
      `/v1/datagram-proxies/${EggchaosClient.quote(name)}`,
      undefined,
      options,
    );
  }

  listDatagramFaults(proxy: string, options?: RequestOptions): Promise<Json> {
    return this.request(
      "GET",
      `/v1/datagram-proxies/${EggchaosClient.quote(proxy)}/faults`,
      undefined,
      options,
    );
  }

  addDatagramFault(
    proxy: string,
    direction: Direction,
    id: string,
    kind: DatagramFaultKind,
    probability?: number,
    options?: RequestOptions,
  ): Promise<Json> {
    const body: Record<string, unknown> = { direction, id, kind };
    if (probability !== undefined) body["probability"] = probability;
    return this.request(
      "POST",
      `/v1/datagram-proxies/${EggchaosClient.quote(proxy)}/faults`,
      body,
      options,
    );
  }

  getDatagramFault(proxy: string, id: string, options?: RequestOptions): Promise<Json> {
    return this.request(
      "GET",
      `/v1/datagram-proxies/${EggchaosClient.quote(proxy)}/faults/${EggchaosClient.quote(id)}`,
      undefined,
      options,
    );
  }

  patchDatagramFault(
    proxy: string,
    id: string,
    patch: DatagramFaultPatchWire,
    options?: RequestOptions,
  ): Promise<Json> {
    return this.request(
      "PATCH",
      `/v1/datagram-proxies/${EggchaosClient.quote(proxy)}/faults/${EggchaosClient.quote(id)}`,
      patch,
      options,
    );
  }

  deleteDatagramFault(proxy: string, id: string, options?: RequestOptions): Promise<Json> {
    return this.request(
      "DELETE",
      `/v1/datagram-proxies/${EggchaosClient.quote(proxy)}/faults/${EggchaosClient.quote(id)}`,
      undefined,
      options,
    );
  }

  listDatagramAssociations(options?: RequestOptions): Promise<Json> {
    return this.request("GET", "/v1/datagram-associations", undefined, options);
  }

  getDatagramAssociation(id: number, options?: RequestOptions): Promise<Json> {
    return this.request(
      "GET",
      `/v1/datagram-associations/${Math.trunc(id)}`,
      undefined,
      options,
    );
  }

  killDatagramAssociation(id: number, options?: RequestOptions): Promise<Json> {
    return this.request(
      "DELETE",
      `/v1/datagram-associations/${Math.trunc(id)}`,
      undefined,
      options,
    );
  }
}
