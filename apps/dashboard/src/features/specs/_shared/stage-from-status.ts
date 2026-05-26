import type { Stage, Outcome, Flags, SpecState } from "@/lib/types/specs";

/**
 * Project a legacy kebab-case `SpecCard.status` string into the canonical
 * `SpecState` (stage / outcome / flags) the `StageBullet` consumes.
 *
 * The dashboard list still fans out `SpecCard` rows whose `status` is the flat
 * `mustard_core::SpecStatus` spelling (`planning`, `implementing`, `reviewing`,
 * `qa`, `closed-followup`, `completed`, `cancelled`, `abandoned`, `blocked`,
 * `wave-failed`, `no-events`). This mirrors the `SpecStatus → SpecState` lift
 * in `packages/core/src/model/view/spec.rs` so the bullet renders the same
 * stage the core projection would.
 */
export function stateFromStatus(status: string): SpecState {
  let stage: Stage = "plan";
  let outcome: Outcome = "active";
  const flags: Flags = { blocked: false, wave_failed: false, followup_open: false };

  switch (status) {
    case "no-events":
    case "planning":
      stage = "plan";
      break;
    case "implementing":
      stage = "execute";
      break;
    case "reviewing":
    case "qa":
      stage = "qa-review";
      break;
    case "closed-followup":
      stage = "close";
      flags.followup_open = true;
      break;
    case "completed":
    case "closed":
      stage = "close";
      outcome = "completed";
      break;
    case "cancelled":
      stage = "close";
      outcome = "cancelled";
      break;
    case "abandoned":
      stage = "close";
      outcome = "abandoned";
      break;
    // Wave 4 of deep-refactor (2026-05-25) — terminal outcomes split out so
    // each renders its own coloured badge instead of collapsing into
    // "cancelled".
    case "superseded":
      stage = "close";
      outcome = "superseded";
      break;
    case "absorbed":
      stage = "close";
      outcome = "absorbed";
      break;
    case "blocked":
      // Qualifier wins over stage in the legacy projection; surface it on the
      // earliest meaningful stage so the pause badge shows.
      stage = "plan";
      flags.blocked = true;
      break;
    case "wave-failed":
      stage = "execute";
      flags.wave_failed = true;
      break;
    // `active`/`closed` legacy fallbacks plus anything unknown map to plan.
    case "active":
      stage = "execute";
      break;
    default:
      stage = "plan";
  }

  return { stage, outcome, flags };
}

/**
 * The top-level filter bucket a `SpecState` belongs to, used by the discrete
 * `Ativas / Suspeitas / Encerradas` pills on `/specs`.
 *
 * - `ativas` — running pipelines (`outcome === "active"`, not abandoned-ish).
 * - `suspeitas` — flagged-but-active: blocked or a failed wave (needs a look).
 * - `encerradas` — any terminal outcome (completed / cancelled / abandoned).
 */
export type SpecFilterBucket = "ativas" | "suspeitas" | "encerradas";

export function filterBucket(state: SpecState): SpecFilterBucket {
  if (state.outcome !== "active") return "encerradas";
  if (state.flags.blocked || state.flags.wave_failed) return "suspeitas";
  return "ativas";
}
