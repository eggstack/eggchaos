/** Real-server integration: the equivalent flow through the TS client.
 * Requires EGGCHAOS_BASE_URL (set by scripts/qualify_language_clients.sh). */
import { describe, it } from "node:test";
import assert from "node:assert/strict";

import { EggchaosClient } from "../src/client.js";
import { EggchaosError } from "../src/errors.js";

const BASE_URL = process.env["EGGCHAOS_BASE_URL"];

describe("typescript live client", { skip: !BASE_URL }, () => {
  it("drives the full stream and datagram flow", async () => {
    const client = new EggchaosClient({ baseUrl: BASE_URL! });
    const health = (await client.health()) as { running: boolean };
    assert.equal(health.running, true);
    const version = (await client.version()) as { api: string };
    assert.equal(version.api, "v1");

    const created = (await client.createProxy({
      name: "ts-client",
      listen: "127.0.0.1:0",
      upstream: "127.0.0.1:9",
    })) as { proxy: { name: string } };
    assert.equal(created.proxy.name, "ts-client");
    const fault = (await client.addFault("ts-client", "downstream", "lag", {
      type: "latency",
      delay_ns: 1_000_000,
    })) as { fault: { id: string } };
    assert.equal(fault.fault.id, "lag");
    const patched = (await client.patchFault("ts-client", "lag", {
      probability: 0.25,
    })) as { fault: { probability: number } };
    assert.equal(patched.fault.probability, 0.25);
    const faults = (await client.listFaults("ts-client")) as {
      downstream: Array<{ id: string }>;
    };
    assert.ok(faults.downstream.some((entry) => entry.id === "lag"));
    assert.ok(Array.isArray(await client.listConnections()));
    assert.ok(Array.isArray(await client.history()));
    assert.match(await client.metricsText(), /eggchaos_/);
    const run = (await client.applyScenario({ version: 1, seed: 1, events: [] })) as {
      run_id: number;
    };
    assert.equal(
      ((await client.getScenario(run.run_id)) as typeof run).run_id,
      run.run_id,
    );
    const schedule = {
      version: 2,
      seed: 7,
      execution_key: 11,
      isolation: "strict",
      cleanup: "restore-initial",
      phases: [
        {
          name: "probe",
          duration_ns: 1_000_000,
          actions: [
            { type: "remove-fault", proxy: "ts-client", direction: "downstream", id: "lag" },
          ],
        },
      ],
    };
    assert.ok(
      ((await client.validateSchedule(schedule)) as { schedule_fingerprint: string })
        .schedule_fingerprint,
    );
    assert.ok(Array.isArray(((await client.compileSchedule(schedule)) as { events: unknown[] }).events));
    assert.equal(
      ((await client.compileSchedule(schedule)) as { events: unknown[] }).events.length,
      1,
    );
    assert.equal(
      ((await client.deleteFault("ts-client", "lag")) as { deleted: boolean }).deleted,
      true,
    );
    const loss = (await client.addFault("ts-client", "downstream", "loss", {
      type: "stream-loss",
      loss_rate: 0.25,
      correlation: 0.1,
    })) as { fault: { id: string } };
    assert.equal(loss.fault.id, "loss");
    const lossView = ((await client.getFault("ts-client", "loss")) as {
      fault: { kind: { type: string; loss_rate: number; correlation: number } };
    }).fault;
    assert.equal(lossView.kind.type, "stream-loss");
    assert.equal(lossView.kind.loss_rate, 0.25);
    assert.equal(lossView.kind.correlation, 0.1);
    assert.equal(
      ((await client.deleteFault("ts-client", "loss")) as { deleted: boolean }).deleted,
      true,
    );
    assert.equal(
      ((await client.deleteProxy("ts-client")) as { deleted: boolean }).deleted,
      true,
    );
    assert.equal(((await client.reset()) as { reset: boolean }).reset, true);

    const dgram = (await client.createDatagramProxy({
      name: "ts-dns",
      listen: "127.0.0.1:0",
      upstream: "127.0.0.1:9",
    })) as { proxy: { name: string } };
    assert.equal(dgram.proxy.name, "ts-dns");
    const dFault = (await client.addDatagramFault("ts-dns", "upstream", "loss", {
      type: "loss",
    })) as { fault: { id: string } };
    assert.equal(dFault.fault.id, "loss");
    assert.ok(Array.isArray(await client.listDatagramAssociations()));
    await assert.rejects(client.getDatagramProxy("absent"), (error: unknown) => {
      assert.ok(error instanceof EggchaosError);
      assert.equal(error.status, 404);
      assert.equal(error.code, "not_found");
      return true;
    });
    assert.equal(
      ((await client.deleteDatagramFault("ts-dns", "loss")) as { deleted: boolean }).deleted,
      true,
    );
    assert.equal(
      ((await client.deleteDatagramProxy("ts-dns")) as { deleted: boolean }).deleted,
      true,
    );
  });

  it("propagates auth failures and cancellation", { skip: !process.env["EGGCHAOS_AUTH_URL"] }, async () => {
    const authUrl = process.env["EGGCHAOS_AUTH_URL"]!;
    const authed = new EggchaosClient({ baseUrl: authUrl, token: "wrong-token" });
    await assert.rejects(authed.health(), (error: unknown) => {
      assert.ok(error instanceof EggchaosError);
      assert.equal(error.status, 403);
      assert.ok(!String(error).includes("wrong-token"));
      return true;
    });
    const authedOk = new EggchaosClient({ baseUrl: authUrl, token: "correct-token" });
    assert.equal(((await authedOk.health()) as { running: boolean }).running, true);
    const client = new EggchaosClient({ baseUrl: BASE_URL! });
    const controller = new AbortController();
    controller.abort();
    await assert.rejects(client.health({ signal: controller.signal }));
  });
});
