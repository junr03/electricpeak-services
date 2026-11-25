import { describe, expect, it } from "vitest";
import pg from "pg";

const host = process.env.SOURCE_TEST_HOST;
describe.skipIf(!host)("operator-authorized PostgreSQL source", () => {
  it("discovers and queries a TLS source using a role that cannot write", async () => {
    const { testPostgresConnection, listPostgresTables, queryPostgres } = await import("../lib/sources/postgres/client");
    const target = {
      host: host!, port: 5432, database: "transactions", user: "opendashboards_reader",
      password: process.env.SOURCE_TEST_PASSWORD!, sslMode: "verify-full" as const,
    };
    if (process.env.SOURCE_TEST_EXPECT_BLOCKED === "true") {
      await expect(testPostgresConnection(target)).rejects.toMatchObject({ code: "host_blocked" });
      return;
    }
    expect((await testPostgresConnection(target)).canWrite).toBe(false);
    const tables = await listPostgresTables(target, "transaction_import");
    expect(tables.rows.some((row) => row.table === "transactions")).toBe(true);
    expect(tables.rows.some((row) => row.table === "future_import")).toBe(true);
    const result = await queryPostgres(target, "SELECT sum(amount) AS total FROM transaction_import.transactions", [], 5);
    expect(result.rows).toEqual([{ total: 12.34 }]);
    await expect(queryPostgres(target, "DELETE FROM transaction_import.transactions", [], 5)).rejects.toThrow();
    await expect(testPostgresConnection({ ...target, host: process.env.SOURCE_TEST_ADDRESS! })).rejects.toMatchObject({ code: "host_blocked" });
    await expect(testPostgresConnection({ ...target, host: "metadata-db" })).rejects.toMatchObject({ code: "host_blocked" });

    // Check database grants independently of the application's read-only wrapper.
    const raw = new pg.Client({ ...target, ssl: { rejectUnauthorized: true } });
    try {
      await raw.connect();
      await raw.query("SET default_transaction_read_only = off");
      await expect(raw.query("INSERT INTO transaction_import.transactions (amount) VALUES (1)")).rejects.toMatchObject({ code: "42501" });
      await expect(raw.query("CREATE TABLE transaction_import.forbidden (id int)")).rejects.toMatchObject({ code: "42501" });
    } finally { await raw.end(); }
  });
});
