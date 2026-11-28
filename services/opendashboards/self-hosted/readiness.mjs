import { setTimeout as delay } from "node:timers/promises";

/** Poll in the application container; independent of the host orchestrator. */
export async function waitForModels({ baseUrl, models, apiKey, timeoutMs = 3_600_000 }) {
  const deadline = Date.now() + timeoutMs;
  const url = new URL("models", baseUrl.replace(/\/?$/, "/"));
  while (Date.now() < deadline) {
    try {
      const response = await fetch(url, {
        headers: apiKey ? { Authorization: `Bearer ${apiKey}` } : {},
        signal: AbortSignal.timeout(Math.max(1, Math.min(5000, deadline - Date.now()))),
      });
      if (response.ok) {
        const body = await response.json();
        const available = new Set((body.data ?? []).map((model) => model.id));
        if (models.every((model) => available.has(model))) return;
      }
    } catch { /* Server may be starting or still pulling weights. */ }
    await delay(Math.min(1000, Math.max(0, deadline - Date.now())));
  }
  throw new Error("Configured inference models did not become ready before the startup timeout");
}

export async function waitForDatabase(databaseUrl, timeoutMs = 90_000) {
  const { default: pg } = await import("pg");
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const client = new pg.Client({ connectionString: databaseUrl, connectionTimeoutMillis: 2000 });
    try {
      await client.connect();
      await client.query("SELECT 1");
      return;
    } catch { /* The database container may still be initializing. */ }
    finally { await client.end().catch(() => {}); }
    await delay(Math.min(1000, Math.max(0, deadline - Date.now())));
  }
  throw new Error("Application database did not become ready before the startup timeout");
}
