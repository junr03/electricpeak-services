import { readFileSync } from "node:fs";

import type { PrivateDestination } from "./settings";
import { privateDestinationProblems } from "../lib/net/destination-policy";

/** An operator-mounted file uses the same validation as typed network settings. */
export function readOperatorPrivateDestinations(path: string | undefined): PrivateDestination[] | null {
  if (path === undefined) return null;
  if (!path.trim()) throw new Error("Private destinations file path must not be empty");
  const rules: unknown = JSON.parse(readFileSync(path, "utf8"));
  const problems = privateDestinationProblems(rules as PrivateDestination[]);
  if (problems.length) throw new Error(`Invalid operator private destinations: ${problems.join("; ")}`);
  return rules as PrivateDestination[];
}

// Read once per process. Permission changes require an application restart.
export const operatorPrivateDestinations = readOperatorPrivateDestinations(
  process.env.OPENDASHBOARDS_PRIVATE_DESTINATIONS_FILE,
);
