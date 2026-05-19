"use strict";

/**
 * spec-sections.js — single source of truth for spec markdown section headings.
 *
 * Mustard specs may be written in English (`## Files`) or Portuguese
 * (`## Arquivos`). Parsers historically hardcoded the EN heading and silently
 * failed on PT specs. This module centralizes the canonical key ↔ language
 * variant mapping so every parser resolves headings the same way.
 *
 * Transcribed from the Header Translation Table in
 * `templates/refs/feature/spec-language.md`.
 *
 * CommonJS, Node.js built-ins only — safe to require from hooks and scripts.
 */

/**
 * Canonical section key → ordered list of accepted heading names.
 * Index 0 is the canonical EN name; the LAST entry is the canonical PT name.
 * Any middle entries are additional EN aliases (e.g. `tasks` accepts both
 * `Tasks` and `Checklist`). `sectionHeading` relies on this ordering.
 *
 * @type {Record<string, string[]>}
 */
const SECTIONS = {
  context: ["Context", "Contexto"],
  summary: ["Summary", "Resumo"],
  boundaries: ["Boundaries", "Limites"],
  files: ["Files", "Arquivos"],
  rootCause: ["Root cause", "Causa raiz"],
  tasks: ["Tasks", "Checklist", "Tarefas"],
  acceptanceCriteria: ["Acceptance Criteria", "Critérios de Aceitação"],
  nonGoals: ["Non-Goals", "Não-Objetivos"],
  concerns: ["Concerns", "Preocupações"],
  decisions: ["Decisions", "Decisões não-óbvias"],
  dependencies: ["Dependencies", "Dependências"],
  entityInfo: ["Entity Info", "Informações da Entidade"],
  symptom: ["Symptom", "Sintoma"],
  // NOTE: the `## PRD` and `## Plano` two-layer divider headings are
  // intentionally absent. They group subsections for human readability and
  // are consumed by no parser. A `plan` key was removed because
  // `findSection(md, "plan")` would resolve to the `## Plano` divider and
  // return the entire Plano layer, not a content section. See the two-layer
  // note in templates/refs/feature/spec-language.md.
};

/**
 * Escape regex metacharacters in a literal string.
 * @param {string} str
 * @returns {string}
 */
function escapeRegex(str) {
  return str.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * Build a RegExp matching a `## ` heading for the given canonical key.
 *
 * The regex is case-insensitive and multiline, anchored to the start of a
 * line. It tolerates an arbitrary suffix after the heading name (e.g.
 * `## Acceptance Criteria (this pipeline)`).
 *
 * @param {string} key — a canonical key present in SECTIONS.
 * @returns {RegExp}
 * @throws {Error} when the key is unknown.
 */
function headingRegex(key) {
  const variants = SECTIONS[key];
  if (!variants) {
    throw new Error(`spec-sections: unknown section key "${key}"`);
  }
  // Longest variants first so e.g. "Critérios de Aceitação" wins over a
  // hypothetical shorter prefix; \b ends the name, .* allows a suffix.
  const alternation = variants
    .slice()
    .sort((a, b) => b.length - a.length)
    .map(escapeRegex)
    .join("|");
  return new RegExp(`^##\\s+(?:${alternation})\\b.*$`, "im");
}

/**
 * Locate a spec section by canonical key.
 *
 * @param {string} markdown — full spec markdown text.
 * @param {string} key — a canonical key present in SECTIONS.
 * @returns {{start:number,end:number,content:string}|null}
 *   `start` is the index of the heading line, `end` is the index just before
 *   the next `## ` heading (or end-of-file). `content` is the slice between
 *   them, including the heading line. Returns null when the section is absent.
 */
function findSection(markdown, key) {
  if (typeof markdown !== "string" || markdown.length === 0) return null;
  const re = headingRegex(key);
  const match = re.exec(markdown);
  if (!match) return null;

  const start = match.index;
  // Find the next `## ` heading after this one.
  const nextRe = /^##\s+/im;
  const rest = markdown.slice(start + match[0].length);
  const nextMatch = nextRe.exec(rest);
  const end =
    nextMatch != null
      ? start + match[0].length + nextMatch.index
      : markdown.length;

  return { start, end, content: markdown.slice(start, end) };
}

/**
 * Return the heading string a generator should write for a given key/language.
 *
 * @param {string} key — a canonical key present in SECTIONS.
 * @param {"pt"|"en"} [lang="en"]
 * @returns {string} the heading text WITHOUT the leading `## `.
 * @throws {Error} when the key is unknown.
 */
function sectionHeading(key, lang) {
  const variants = SECTIONS[key];
  if (!variants) {
    throw new Error(`spec-sections: unknown section key "${key}"`);
  }
  // Index 0 is the canonical EN name; the last entry is the canonical PT name.
  const usePt = lang === "pt" && variants.length > 1;
  return usePt ? variants[variants.length - 1] : variants[0];
}

module.exports = { SECTIONS, headingRegex, findSection, sectionHeading };
