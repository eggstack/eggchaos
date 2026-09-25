/** Cross-language fixtures: equivalent TS inputs serialize to the exact
 * shared native JSON wire bodies (mock transport, no server). */
import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { EggchaosClient } from "../src/client.js";

const here = dirname(fileURLToPath(import.meta.url));

function loadFixtures(): Array<{ name: string; method: string; path: string; wire: unknown }> {
  const override = process.env["EGGCHAOS_FIXTURES"];
  const candidates = [
    ...(override ? [override] : []),
    resolve(here, "..", "..", "..", "_contract", "cross_language_fixtures.json"),
    resolve(here, "..", "..", "_contract", "cross_language_fixtures.json"),
  ];
  for (const candidate of candidates) {
    try {
      return JSON.parse(readFileSync(candidate, "utf-8")).cases;
    } catch {
      // try next
    }
  }
  throw new Error("cross-language fixtures not found");
}

const fixtures = loadFixtures();

function wire(name: string): unknown {
  const found = fixtures.find((entry) => entry.name === name);
  assert.ok(found, name);
  return found!.wire;
}

interface Captured {
  method: string;
  path: string;
  body: unknown;
}

function mockClient(captured: Captured[]): EggchaosClient {
  const base = "http://127.0.0.1:9";
  return new EggchaosClient({
    baseUrl: base,
    fetch: (async (url: string | URL | Request, init?: RequestInit) => {
      const text = init?.body ? String(init.body) : "";
      captured.push({
        method: init?.method ?? "GET",
        path: String(url).slice(base.length),
        body: text ? JSON.parse(text) : undefined,
      });
      return new Response("{}", {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }) as typeof fetch,
  });
}

describe("typescript cross-language fixtures", () => {
  it("serializes proxy and fault bodies to shared wire", async () => {
    const captured: Captured[] = [];
    const client = mockClient(captured);
    await client.createProxy({ name: "redis", listen: "127.0.0.1:0", upstream: "127.0.0.1:6379" });
    await client.addFault("redis", "downstream", "lag", {
      type: "latency",
      delay_ns: 200_000_000,
      jitter_ns: 0,
      max_buffer_bytes: 65536,
    }, 0.5);
    await client.addFault("redis", "upstream", "cap", { type: "bandwidth" });
    await client.addDatagramFault("dns", "upstream", "loss", { type: "loss" }, 0.25);
    await client.applyScenario({
      version: 1,
      seed: 7,
      events: [
        {
          at_ms: 25,
          action: { type: "remove-fault", proxy: "cache", direction: "upstream", id: "delay" },
        },
      ],
    });
    await client.validateSchedule({
      version: 2,
      seed: 7,
      execution_key: 11,
      isolation: "strict",
      cleanup: "restore-initial",
      phases: [],
    });
    const byPath = new Map(captured.map((entry) => [`${entry.method} ${entry.path}`, entry.body]));
    assert.deepEqual(byPath.get("POST /v1/proxies"), wire("proxy_create"));
    // Two faults share one path; assert both from the raw capture order.
    assert.deepEqual(captured[1].body, wire("stream_fault_latency"));
    assert.deepEqual(captured[2].body, wire("stream_fault_bandwidth_defaults"));
    assert.deepEqual(captured[3].body, wire("datagram_fault_loss"));
    assert.deepEqual(captured[4].body, wire("scenario_v1_apply"));
    assert.deepEqual(captured[5].body, wire("schedule_v2_validate"));
  });

  it("omits absent optionals so server defaults apply", async () => {
    const captured: Captured[] = [];
    const client = mockClient(captured);
    await client.createDatagramProxy({
      name: "dns",
      listen: "127.0.0.1:0",
      upstream: "127.0.0.1:9",
    });
    assert.deepEqual(captured[0].body, {
      name: "dns",
      listen: "127.0.0.1:0",
      upstream: "127.0.0.1:9",
    });
  });
});
