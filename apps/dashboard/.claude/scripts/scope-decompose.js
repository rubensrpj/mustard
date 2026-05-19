#!/usr/bin/env node

/**
 * scope-decompose.js
 *
 * Decides whether a feature spec should be decomposed into multiple waves.
 * Reads signals from stdin (JSON), emits decision to stdout (JSON).
 *
 * Input (stdin):
 *   {
 *     fileCount: number,          // arquivos em "## Files" do spec
 *     layerCount: number,         // camadas distintas tocadas
 *     newEntityCount: number,     // entidades novas
 *     estimatedTouchPoints: number, // imports/refs cruzados (opcional)
 *     knowledgeMatches: [         // entradas heavy-pipeline/high-hook-retry do knowledge.json
 *       { id: string, type: string, scope: object }
 *     ]
 *   }
 *
 * Output (stdout):
 *   { decompose: boolean, reason: string, signals: {...} }
 *
 * Fail-open: on any error, emits { decompose: false, reason: "error-fallback" }
 * and exits 0.
 */

"use strict";

function readStdin() {
  return new Promise((resolve) => {
    let data = "";
    process.stdin.setEncoding("utf8");
    process.stdin.on("data", (chunk) => {
      data += chunk;
    });
    process.stdin.on("end", () => resolve(data));
    process.stdin.on("error", () => resolve(""));
  });
}

function decide(signals) {
  const {
    fileCount = 0,
    layerCount = 0,
    newEntityCount = 0,
    estimatedTouchPoints = 0,
    knowledgeMatches = [],
  } = signals;

  const hasHistoricalMatch = Array.isArray(knowledgeMatches) && knowledgeMatches.length > 0;

  if (hasHistoricalMatch) {
    return {
      decompose: true,
      reason: `history-match:${knowledgeMatches[0].id || "unknown"}`,
      signals: { fileCount, layerCount, newEntityCount, estimatedTouchPoints, historicalMatches: knowledgeMatches.length },
    };
  }

  if (layerCount >= 2) {
    return {
      decompose: true,
      reason: "multi-layer",
      signals: { fileCount, layerCount, newEntityCount, estimatedTouchPoints, historicalMatches: 0 },
    };
  }

  if (fileCount > 10 && newEntityCount >= 2) {
    return {
      decompose: true,
      reason: "wide-and-new-entities",
      signals: { fileCount, layerCount, newEntityCount, estimatedTouchPoints, historicalMatches: 0 },
    };
  }

  return {
    decompose: false,
    reason: "single-layer",
    signals: { fileCount, layerCount, newEntityCount, estimatedTouchPoints, historicalMatches: 0 },
  };
}

async function main() {
  try {
    const raw = await readStdin();
    let signals = {};
    if (raw.trim()) {
      signals = JSON.parse(raw);
    }
    const decision = decide(signals);
    process.stdout.write(JSON.stringify(decision));
  } catch (_err) {
    process.stdout.write(JSON.stringify({ decompose: false, reason: "error-fallback" }));
  }
}

main();
