"use strict";

/**
 * wave-lib.js — shared helpers for wave-* and exec-rewave-check scripts.
 *
 * Extracted from exec-rewave-check.js / wave-size-check.js / wave-dependency.js
 * (all three had byte-for-byte identical detectRole; exec-rewave and wave-size
 * also had identical parseFiles).
 */

/**
 * Classify a file path into a coarse architectural role.
 * @param {string} filePath
 * @returns {"schema"|"api"|"ui"|"test"|"lib"}
 */
function detectRole(filePath) {
  const lower = filePath.toLowerCase();
  if (/(schema|migration|entity|model|drizzle|prisma)/.test(lower)) return "schema";
  if (/(api|controller|route|endpoint|handler|service)/.test(lower)) return "api";
  if (/(ui|component|view|page|screen|widget)/.test(lower)) return "ui";
  if (/(test|spec|__tests__)/.test(lower)) return "test";
  return "lib";
}

/**
 * Parse the `## Files` section of a spec and return the listed paths.
 * Returns null when the section is absent; returns [] when present but empty.
 * @param {string} specText
 * @returns {string[]|null}
 */
function parseFilesSection(specText) {
  const lines = specText.split("\n");
  const start = lines.findIndex((l) => /^##\s+Files/.test(l));
  if (start === -1) return null;

  const paths = [];
  for (let i = start + 1; i < lines.length; i++) {
    const line = lines[i].trim();
    if (/^##\s/.test(line)) break; // next section
    // match "- path" or "- `path`" bullets
    const m = line.match(/^-\s+`?([^\s`]+)`?/);
    if (m && m[1] && !m[1].startsWith("#")) {
      paths.push(m[1]);
    }
  }
  return paths;
}

module.exports = { detectRole, parseFilesSection };
