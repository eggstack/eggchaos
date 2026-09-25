/** Contract drift: generated tables match the shared snapshot and every
 * operation maps to a real client method. No server needed. */
import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  DATAGRAM_FAULT_TAGS,
  OPERATION_METHODS,
  OPERATIONS,
  SCENARIO_ACTION_TAGS,
  STREAM_FAULT_TAGS,
} from "../src/generated.js";
import { EggchaosClient } from "../src/client.js";
import { decodeDatagramFault, decodeStreamFault } from "../src/models.js";
import { EggchaosContractError } from "../src/errors.js";

const here = dirname(fileURLToPath(import.meta.url));

function findSnapshot(): string {
  const override = process.env["EGGCHAOS_CONTRACT_SNAPSHOT"];
  if (override) return override;
  // dist/tests/*.test.js -> bindings/_contract/operations.json
  const candidates = [
    resolve(here, "..", "..", "..", "_contract", "operations.json"),
    resolve(here, "..", "..", "_contract", "operations.json"),
  ];
  for (const candidate of candidates) {
    try {
      readFileSync(candidate, "utf-8");
      return candidate;
    } catch {
      // try next
    }
  }
  throw new Error("contract snapshot not found");
}

const snapshot = JSON.parse(readFileSync(findSnapshot(), "utf-8"));

describe("typescript contract", () => {
  it("mirrors the shared snapshot", () => {
    const normalized = OPERATIONS.map((op) => ({
      method: op.method,
      operation_id: op.operationId,
      path: op.path,
    }));
    assert.deepEqual(normalized, snapshot.operations);
    assert.deepEqual([...STREAM_FAULT_TAGS], snapshot.stream_fault_tags);
    assert.deepEqual([...DATAGRAM_FAULT_TAGS], snapshot.datagram_fault_tags);
    assert.deepEqual([...SCENARIO_ACTION_TAGS], snapshot.scenario_action_tags);
  });

  it("covers every operation with a client method", () => {
    const client = new EggchaosClient({ baseUrl: "http://127.0.0.1:9" });
    const ids = new Set(OPERATIONS.map((op) => op.operationId));
    assert.deepEqual(new Set(Object.keys(OPERATION_METHODS)), ids);
    for (const [operationId, method] of Object.entries(OPERATION_METHODS)) {
      assert.equal(
        typeof (client as unknown as Record<string, unknown>)[method],
        "function",
        operationId,
      );
    }
  });

  it("decodes discriminated faults and rejects unknown tags", () => {
    assert.equal(
      decodeStreamFault({ type: "limit-data", bytes: 3 }).type,
      "limit-data",
    );
    assert.equal(decodeDatagramFault({ type: "loss" }).type, "loss");
    assert.throws(
      () => decodeStreamFault({ type: "packet-loss" }),
      EggchaosContractError,
    );
    assert.throws(
      () => decodeDatagramFault({ type: "latency" }),
      EggchaosContractError,
    );
  });

  it("rejects userinfo and non-http base urls without leaking tokens", () => {
    assert.throws(() => new EggchaosClient({ baseUrl: "http://u:p@h:1" }));
    assert.throws(() => new EggchaosClient({ baseUrl: "ftp://h:1" }));
  });
});
