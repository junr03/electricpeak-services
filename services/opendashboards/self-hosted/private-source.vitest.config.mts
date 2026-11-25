import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

export default defineConfig({
  resolve: { alias: {
    "@": fileURLToPath(new URL("../", import.meta.url)),
    "server-only": fileURLToPath(new URL("./server-only-stub.mjs", import.meta.url)),
  } },
  test: { include: ["self-hosted/private-source.integration.ts"] },
});
