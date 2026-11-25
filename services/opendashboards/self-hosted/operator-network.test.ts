import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";

import { readOperatorPrivateDestinations } from "../config/operator-network";
import { isAllowedDestinationAddress } from "../lib/net/destination-policy";

const folders: string[] = [];
function file(value: unknown): string {
  const folder = mkdtempSync(join(tmpdir(), "dashboards-network-"));
  folders.push(folder);
  const path = join(folder, "destinations.json");
  writeFileSync(path, JSON.stringify(value));
  return path;
}
afterEach(() => { for (const folder of folders.splice(0)) rmSync(folder, { recursive: true }); });

describe("operator private destination file", () => {
  it("preserves typed settings when no file is configured and permits an explicit empty list", () => {
    expect(readOperatorPrivateDestinations(undefined)).toBeNull();
    expect(readOperatorPrivateDestinations(file([]))).toEqual([]);
  });

  it("authorizes only the exact private host, port and DNS bound", () => {
    const rules = readOperatorPrivateDestinations(file([{ host: "analytics-db", port: 5432, addresses: ["172.30.0.5/32"] }]))!;
    expect(isAllowedDestinationAddress("analytics-db", 5432, "172.30.0.5", rules)).toBe(true);
    for (const [host, port, address] of [
      ["other-db", 5432, "172.30.0.5"],
      ["analytics-db", 5433, "172.30.0.5"],
      ["analytics-db", 5432, "172.30.0.6"],
      ["analytics-db", 5432, "169.254.169.254"],
    ] as const) expect(isAllowedDestinationAddress(host, port, address, rules)).toBe(false);
  });

  it("fails closed for missing, malformed, broad, duplicate and metadata rules", () => {
    expect(() => readOperatorPrivateDestinations("")).toThrow();
    expect(() => readOperatorPrivateDestinations("/does-not-exist/destinations.json")).toThrow();
    const malformed = file([]);
    writeFileSync(malformed, "{");
    expect(() => readOperatorPrivateDestinations(malformed)).toThrow();
    for (const rules of [
      null, {},
      [{ host: "*.internal", port: 5432, addresses: ["172.30.0.0/24"] }],
      [{ host: "analytics-db", port: 5432 }],
      [{ host: "analytics-db", port: 5432, addresses: ["0.0.0.0/0"] }],
      [{ host: "169.254.169.254", port: 5432 }],
      [{ host: "127.0.0.1", port: 5432 }, { host: "127.0.0.1", port: 5432 }],
    ]) expect(() => readOperatorPrivateDestinations(file(rules))).toThrow();
  });
});
