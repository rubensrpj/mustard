import type { SpecRow } from "@/lib/dashboard";

/** Wave number parsed from a child name like `wave-3-fullstack` → 3. */
export function waveNumber(name: string): number | null {
  const m = name.match(/wave-?(\d+)/i);
  if (!m) return null;
  const n = Number.parseInt(m[1], 10);
  return Number.isFinite(n) ? n : null;
}

/** Role suffix of a wave child name (`wave-3-fullstack` → `fullstack`). */
export function waveRole(name: string): string {
  return name.replace(/^wave-?\d+-?/i, "") || "—";
}

export type WaveState = "done" | "active" | "pending" | "cancelled";

/**
 * Estado de uma wave, derivado do `status`/`phase` do seu próprio `spec.md`.
 * O `bucket` de uma wave-filha reflete a pasta do plano-mãe (não o estado
 * individual da wave), por isso não é usado aqui — `status` é a verdade.
 */
export function waveState(row: SpecRow): WaveState {
  const s = (row.status ?? "").toLowerCase().trim();
  const p = (row.phase ?? "").toUpperCase().trim();
  if (s.includes("cancel")) return "cancelled";
  if (s === "completed" || s === "done" || s === "closed") return "done";
  if (
    s === "queued" ||
    s === "pending" ||
    s === "backlog" ||
    s === "blocked" ||
    s === "deferred" ||
    s === "draft"
  ) {
    return "pending";
  }
  if (s) return "active";
  // `status` vazio — cai pra `phase`.
  if (p === "CLOSE") return "done";
  if (p) return "active";
  return "pending";
}

const WAVE_STATE_LABEL: Record<WaveState, string> = {
  done: "concluída",
  active: "em andamento",
  pending: "pendente",
  cancelled: "cancelada",
};

/** Rótulo em português de um `WaveState` — usado em tooltips e legenda. */
export function waveStateLabel(state: WaveState): string {
  return WAVE_STATE_LABEL[state];
}

export interface WaveFamily {
  /** The wave-plan parent spec name. */
  parentName: string;
  /** Wave children, sorted by wave number. Empty when not a wave plan. */
  waves: SpecRow[];
  /** True when the spec is decomposed into waves. */
  isWavePlan: boolean;
}

/**
 * Resolve a spec's wave-plan family — the parent plan plus every wave child.
 * Works whether the clicked spec is the parent or one of the wave children
 * (the backend sets `parent` on children; parents have `parent: null`).
 */
export function resolveWaveFamily(allSpecs: SpecRow[], specName: string): WaveFamily {
  const row = allSpecs.find((s) => s.name === specName);
  const parentName = row?.parent ?? specName;
  const waves = allSpecs
    .filter((s) => s.parent === parentName)
    .sort((a, b) => (waveNumber(a.name) ?? 0) - (waveNumber(b.name) ?? 0));
  return { parentName, waves, isWavePlan: waves.length > 0 };
}
