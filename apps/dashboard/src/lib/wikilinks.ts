// Wikilink utilities — W5.T5.7 of `2026-05-24-mustard-unification`.
//
// The dashboard does NOT render an internal force-graph of `.claude/spec/**`
// any more (the `/specs` page is a virtualised list, see `pages/Specs.tsx`).
// Instead, every `[[name]]` reference inside a spec markdown opens the
// corresponding note in Obsidian via the `obsidian://open` URI scheme.
//
// The vault path is read from `mustard.json#obsidianVault` (default
// `.claude/.obsidian`); the vault NAME (what Obsidian's URI consumes) is the
// last path segment.
//
// This module is intentionally framework-free — no React, no Tauri, no I/O.
// The dashboard wires it into the markdown renderer via
// `WikilinkText`/`renderWithWikilinks`.

/** Regex that matches `[[anything-but-brackets]]`. Captures the inner text. */
export const WIKILINK_PATTERN = /\[\[([^\[\]]+)\]\]/g;

/** Default vault directory (relative to the project root). The user can
 *  override via `mustard.json#obsidianVault`. */
export const DEFAULT_OBSIDIAN_VAULT_PATH = ".claude/.obsidian";

/** Derive the Obsidian vault NAME (last path component) from a vault PATH.
 *  Tolerates both `/` and `\` separators. */
export function vaultNameFromPath(vaultPath: string): string {
  const trimmed = vaultPath.replace(/[\\/]+$/, "");
  const idx = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
  const tail = idx >= 0 ? trimmed.slice(idx + 1) : trimmed;
  return tail || "mustard";
}

/** Build the `obsidian://open?vault=...&file=...` URI for a wikilink target.
 *  The `file` parameter is URL-encoded so spaces, dashes, and unicode in
 *  spec slugs (`2026-05-24-mustard-unification`) survive intact. */
export function obsidianUri(target: string, vaultName: string): string {
  const enc = (s: string) => encodeURIComponent(s);
  return `obsidian://open?vault=${enc(vaultName)}&file=${enc(target)}`;
}

/** One segment of a tokenised wikilink string — either plain text or a link. */
export type WikilinkSegment =
  | { kind: "text"; text: string }
  | { kind: "link"; target: string; href: string };

/** Tokenise `text` into a sequence of segments, replacing every `[[X]]`
 *  occurrence with a `link` segment. The result is order-preserving so a
 *  React renderer can map it directly to `<span>` / `<a>` children. */
export function tokeniseWikilinks(text: string, vaultName: string): WikilinkSegment[] {
  const out: WikilinkSegment[] = [];
  let lastIdx = 0;
  for (const match of text.matchAll(WIKILINK_PATTERN)) {
    const start = match.index ?? 0;
    if (start > lastIdx) {
      out.push({ kind: "text", text: text.slice(lastIdx, start) });
    }
    const target = match[1];
    out.push({
      kind: "link",
      target,
      href: obsidianUri(target, vaultName),
    });
    lastIdx = start + match[0].length;
  }
  if (lastIdx < text.length) {
    out.push({ kind: "text", text: text.slice(lastIdx) });
  }
  // Optimisation: a single-text result (no matches) can be skipped by the
  // caller — keep the segment so the renderer stays uniform.
  return out;
}
