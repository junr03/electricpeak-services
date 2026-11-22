import { defineConfig } from "vitest/config";

export default defineConfig({ test: { include: ["self-hosted/*.test.ts"] } });
