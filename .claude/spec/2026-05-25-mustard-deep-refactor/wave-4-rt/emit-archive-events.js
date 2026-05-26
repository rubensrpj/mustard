#!/usr/bin/env node
// W4 of 2026-05-25-mustard-deep-refactor — emit pipeline.status for every
// archived spec under ~/.mustard-backups/2026-05-25-specs-archive so the
// telemetry.db reflects the final outcome.
//
// Mapping (per the wave spec):
//   2026-05-24-mustard-unification              -> Completed
//   2026-05-21-mustard-v1-installer-and-update  -> Cancelled
//   2026-05-20-dashboard-prd-ai-lapidator       -> Cancelled
//   *-SUPERSEDED                                -> Superseded
//   2026-05-24-config-idioma-tom                -> Absorbed
//   2026-05-24-meta-sidecar                     -> Absorbed
//   2026-05-23-per-spec-event-log-*             -> Absorbed
//   2026-05-23-tf-dashboard-page-primitives     -> Completed
//   2026-05-23-tf-dashboard-ds-tokens-remap     -> Completed
//   2026-05-23-tf-dashboard-eslint-baseline     -> Completed
//   everything else                             -> Completed

const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const ARCHIVE_ROOT = path.join(
  os.homedir(),
  ".mustard-backups",
  "2026-05-25-specs-archive",
);
if (!fs.existsSync(ARCHIVE_ROOT)) {
  console.error(`Archive root not found: ${ARCHIVE_ROOT}`);
  process.exit(1);
}

function resolveOutcome(name) {
  if (name === "2026-05-24-mustard-unification") return "completed";
  if (name === "2026-05-21-mustard-v1-installer-and-update") return "cancelled";
  if (name === "2026-05-20-dashboard-prd-ai-lapidator") return "cancelled";
  if (name.endsWith("-SUPERSEDED")) return "superseded";
  if (name === "2026-05-24-config-idioma-tom") return "absorbed";
  if (name === "2026-05-24-meta-sidecar") return "absorbed";
  if (name.startsWith("2026-05-23-per-spec-event-log-")) return "absorbed";
  if (name === "2026-05-23-tf-dashboard-page-primitives") return "completed";
  if (name === "2026-05-23-tf-dashboard-ds-tokens-remap") return "completed";
  if (name === "2026-05-23-tf-dashboard-eslint-baseline") return "completed";
  return "completed";
}

const specs = fs
  .readdirSync(ARCHIVE_ROOT, { withFileTypes: true })
  .filter((d) => d.isDirectory())
  .map((d) => d.name)
  .sort();

const counts = { completed: 0, cancelled: 0, superseded: 0, absorbed: 0 };
let ok = 0;
let fail = 0;
const failures = [];

for (const spec of specs) {
  const outcome = resolveOutcome(spec);
  const payload = JSON.stringify({
    // `to` is the canonical PipelineStatusPayload field name (see
    // mustard-core::model::event::PipelineStatusPayload). `reason` is a
    // free extra field carried for archive context — readers ignore it.
    to: outcome,
    reason: "archived in deep-refactor consolidation",
  });
  const res = spawnSync(
    "mustard-rt",
    [
      "run",
      "emit-pipeline",
      "--kind",
      "pipeline.status",
      "--spec",
      spec,
      "--payload",
      payload,
    ],
    { stdio: ["ignore", "ignore", "pipe"] },
  );
  if (res.status === 0) {
    ok++;
    counts[outcome]++;
  } else {
    fail++;
    failures.push({ spec, code: res.status, stderr: String(res.stderr ?? "") });
  }
}

console.log("");
console.log(`Emitted ${ok} / ${specs.length} events (${fail} failures)`);
console.log("Outcome breakdown:");
for (const k of Object.keys(counts)) {
  console.log(`  ${k.padEnd(12)} ${counts[k]}`);
}
if (failures.length) {
  console.log("");
  console.log("Failures:");
  for (const f of failures) {
    console.log(`  ${f.spec} (exit ${f.code}): ${f.stderr.trim()}`);
  }
}
