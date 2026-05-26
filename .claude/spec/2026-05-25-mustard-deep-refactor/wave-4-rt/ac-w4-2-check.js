#!/usr/bin/env node
// AC-W4.2 — confirm that mustard.db has a `pipeline.status` event for every
// spec listed in the archive backup. The store is `.claude/.harness/mustard.db`
// inside the Mustard repo root (cwd).

const fs = require("fs");
const os = require("os");
const path = require("path");
const { DatabaseSync } = require("node:sqlite");

const ARCHIVE_ROOT = path.join(
  os.homedir(),
  ".mustard-backups",
  "2026-05-25-specs-archive",
);
const DB_PATH = path.resolve(".claude/.harness/mustard.db");

const specs = fs
  .readdirSync(ARCHIVE_ROOT, { withFileTypes: true })
  .filter((d) => d.isDirectory())
  .map((d) => d.name)
  .sort();

const db = new DatabaseSync(DB_PATH, { readOnly: true });
// pipeline_events schema (mustard-core::sqlite_schema.sql): spec, event, ts, ...
const stmt = db.prepare(
  "SELECT COUNT(*) AS n FROM pipeline_events WHERE kind = 'pipeline.status' AND spec = ?",
);

let withEvent = 0;
const missing = [];
for (const s of specs) {
  const row = stmt.get(s);
  if (row && row.n > 0) {
    withEvent++;
  } else {
    missing.push(s);
  }
}

const distinctRow = db
  .prepare(
    `SELECT COUNT(DISTINCT spec) AS n FROM pipeline_events WHERE kind = 'pipeline.status' AND spec IN (${specs
      .map(() => "?")
      .join(",")})`,
  )
  .get(...specs);
db.close();

console.log(`archive specs:                ${specs.length}`);
console.log(`specs with pipeline.status:   ${withEvent}`);
console.log(`DISTINCT(spec) matches:        ${distinctRow.n}`);
if (missing.length) {
  console.log("");
  console.log(`MISSING (${missing.length}):`);
  for (const m of missing) console.log("  -", m);
  process.exit(1);
}
if (withEvent === specs.length && distinctRow.n === specs.length) {
  console.log("");
  console.log("AC-W4.2 PASS");
  process.exit(0);
}
process.exit(1);
