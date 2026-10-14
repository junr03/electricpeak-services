"use strict";

const STATUS_URL = "data/status.json";
const REFRESH_MS = 15000;
// The collector runs every 30 seconds; several missed runs mean it is down.
const STALE_AFTER_S = 150;

const LABELS = {
  up: "Up", idle: "Idle", degraded: "Degraded", down: "Down", unknown: "Unknown",
};
const BANNERS = {
  operational: ["✓", "All systems operational"],
  degraded: ["!", "Some services need attention"],
  outage: ["✕", "Outage: a core connection is down"],
  stale: ["?", "Status data is stale; the collector may not be running"],
  unknown: ["?", "Status unavailable"],
};

function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function card(name, status, detail) {
  const node = el("div", `card ${status}`);
  node.append(el("h3", "", name), el("div", "state", LABELS[status] || status), el("p", "", detail));
  return node;
}

function render(snapshot, error) {
  const age = snapshot ? Math.max(0, Math.floor(Date.now() / 1000) - snapshot.generated_at) : null;
  const stale = snapshot && age > STALE_AFTER_S;
  const overall = error ? "unknown" : stale ? "stale" : snapshot.overall;
  const [icon, text] = BANNERS[overall] || BANNERS.unknown;

  const banner = document.getElementById("banner");
  banner.className = `banner ${overall}`;
  document.getElementById("banner-icon").textContent = icon;
  document.getElementById("banner-text").textContent = error ? "Status unavailable: the status feed could not be loaded" : text;
  document.getElementById("updated").textContent = snapshot
    ? `Updated ${new Date(snapshot.generated_at * 1000).toLocaleString()}`
    : "Not updated";
  if (!snapshot) return;

  document.getElementById("checks").replaceChildren(
    ...snapshot.checks.map((check) => card(check.name, check.status, check.detail)),
  );

  const attention = snapshot.containers.filter((c) => !["up", "idle"].includes(c.status));
  const quiet = snapshot.containers.filter((c) => ["up", "idle"].includes(c.status));
  document.getElementById("attention-section").hidden = attention.length === 0;
  document.getElementById("attention-list").replaceChildren(
    ...attention.map((c) => card(c.name, c.status, c.detail)),
  );

  const list = document.getElementById("healthy-list");
  if (quiet.length === 0) {
    list.replaceChildren(el("li", "empty", attention.length ? "No healthy services" : "No containers found"));
    return;
  }
  list.replaceChildren(...quiet.map((c) => {
    const item = el("li", c.status, c.name);
    item.title = c.detail;
    return item;
  }));
}

async function refresh() {
  try {
    const response = await fetch(STATUS_URL, { cache: "no-store" });
    if (!response.ok) throw new Error(String(response.status));
    render(await response.json(), false);
  } catch {
    render(null, true);
  }
}

refresh();
setInterval(refresh, REFRESH_MS);
