import assert from "node:assert/strict";
import { createServer } from "node:http";
import { test } from "node:test";
import { waitForModels } from "./readiness.mjs";

async function endpoint(handler, run) {
  const server = createServer(handler);
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  try { await run(`http://127.0.0.1:${server.address().port}/v1`); }
  finally { server.closeAllConnections(); await new Promise((resolve) => server.close(resolve)); }
}

test("waits until all selected models are available, including a separate dashboard model", async () => {
  let calls = 0;
  await endpoint((request, response) => {
    assert.equal(request.url, "/v1/models");
    assert.equal(request.headers.authorization, "Bearer fixture-token");
    response.setHeader("Content-Type", "application/json");
    response.end(JSON.stringify({ data: (++calls === 1 ? ["chat"] : ["chat", "dashboard"]).map((id) => ({ id })) }));
  }, async (baseUrl) => {
    await waitForModels({ baseUrl, models: ["chat", "dashboard"], apiKey: "fixture-token", timeoutMs: 5000 });
    assert.equal(calls, 2);
  });
});

test("accepts an already provisioned model with no retry or authentication header", async () => {
  let calls = 0;
  await endpoint((request, response) => {
    calls++;
    assert.equal(request.headers.authorization, undefined);
    response.end(JSON.stringify({ data: [{ id: "cached" }] }));
  }, async (baseUrl) => {
    await waitForModels({ baseUrl: baseUrl + "/", models: ["cached"], timeoutMs: 500 });
    assert.equal(calls, 1);
  });
});

test("fails startup rather than claiming readiness for a missing model or failed API", async () => {
  for (const status of [200, 503]) {
    await endpoint((request, response) => {
      response.writeHead(status);
      response.end(JSON.stringify({ data: [] }));
    }, async (baseUrl) => {
      await assert.rejects(waitForModels({ baseUrl, models: ["missing"], timeoutMs: 30 }), /startup timeout/);
    });
  }
});
