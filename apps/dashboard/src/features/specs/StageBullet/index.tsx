import { Check, Ban, X, Pause, AlertTriangle } from "lucide-react";
import { cn } from "@/lib/utils";
import type { Stage, Outcome, Flags } from "@/lib/types/specs";

/**
 * StageBullet — a 16x16 SVG ring split into five arcs, one per lifecycle
 * `Stage` (Analyze / Plan / Execute / QaReview / Close). The ring reads
 * "where is this spec in the pipeline?" at a glance:
 *
 *   - arcs *before* the current stage render fully opaque (done),
 *   - the *current* arc pulses (in flight),
 *   - arcs *after* render at 20% opacity (not reached yet).
 *
 * Each arc is coloured by the `--color-phase-{name}` CSS var already declared
 * in `style.css`, so the bullet stays in lockstep with the rest of the phase
 * palette across light/dark themes.
 *
 * When the spec is terminal (`outcome != "active"`), the whole ring is painted
 * in the outcome colour with a central icon — completed (✓, green), cancelled
 * (⊘, amber) or abandoned (⊗, grey). Orthogonal flags add corner adornments:
 * `blocked` → a pause badge, `wave_failed` → an alert triangle.
 *
 * CSS-only animation via `stroke-dashoffset` keeps the component cheap (no JS
 * timers, no re-render churn).
 */

interface StageBulletProps {
  stage: Stage;
  outcome?: Outcome;
  flags?: Partial<Flags>;
  /** Edge length of the square SVG. Defaults to 16; child rows pass 12. */
  size?: number;
  className?: string;
}

// Ordered stages — index drives done/current/future classification.
const STAGES: Stage[] = ["analyze", "plan", "execute", "qa-review", "close"];

// Map each Stage to the phase-name suffix of its `--color-phase-*` CSS var.
// `qa-review` collapses onto the `qa` hue (the theme has no `qa-review` var).
const STAGE_VAR: Record<Stage, string> = {
  analyze: "analyze",
  plan: "plan",
  execute: "execute",
  "qa-review": "qa",
  close: "close",
};

// Terminal-outcome ring colour + central glyph.
const OUTCOME_META: Record<
  Exclude<Outcome, "active">,
  { color: string; Icon: typeof Check }
> = {
  completed: { color: "var(--color-ok, #22c55e)", Icon: Check },
  cancelled: { color: "var(--color-phase-plan, #f59e0b)", Icon: Ban },
  abandoned: { color: "var(--color-muted-foreground, #71717a)", Icon: X },
};

export function StageBullet({
  stage,
  outcome = "active",
  flags,
  size = 16,
  className,
}: StageBulletProps) {
  const isTerminal = outcome !== "active";
  const blocked = flags?.blocked ?? false;
  const waveFailed = flags?.wave_failed ?? false;

  // Geometry. Stroke width scales with size so the 12px child variant stays
  // legible. The radius leaves a half-stroke margin so arcs don't clip.
  const stroke = size <= 12 ? 2 : 2.4;
  const r = (size - stroke) / 2;
  const cx = size / 2;
  const cy = size / 2;
  const circ = 2 * Math.PI * r;
  // Five equal arcs with a small gap so segments read as discrete.
  const gap = circ * 0.04;
  const seg = circ / STAGES.length - gap;

  const currentIdx = STAGES.indexOf(stage);

  return (
    <span
      className={cn("relative inline-flex shrink-0", className)}
      style={{ width: size, height: size }}
      role="img"
      aria-label={
        isTerminal ? `Spec ${outcome}` : `Stage ${stage}`
      }
    >
      <svg
        width={size}
        height={size}
        viewBox={`0 0 ${size} ${size}`}
        // Rotate so the first arc starts at 12 o'clock.
        style={{ transform: "rotate(-90deg)" }}
        aria-hidden
      >
        {isTerminal ? (
          // Terminal: a single full ring in the outcome colour.
          <circle
            cx={cx}
            cy={cy}
            r={r}
            fill="none"
            stroke={OUTCOME_META[outcome as Exclude<Outcome, "active">].color}
            strokeWidth={stroke}
          />
        ) : (
          STAGES.map((s, i) => {
            const done = i < currentIdx;
            const current = i === currentIdx;
            const color = `var(--color-phase-${STAGE_VAR[s]})`;
            const opacity = done ? 1 : current ? 1 : 0.2;
            const offset = -(i * (seg + gap));
            return (
              <circle
                key={s}
                cx={cx}
                cy={cy}
                r={r}
                fill="none"
                stroke={color}
                strokeWidth={stroke}
                strokeLinecap="round"
                strokeDasharray={`${seg} ${circ - seg}`}
                strokeDashoffset={offset}
                opacity={opacity}
                className={current ? "stage-bullet-pulse" : undefined}
                style={
                  current
                    ? ({
                        // The pulse animation eases the dashoffset around the
                        // current arc; defined in style.css.
                        ["--seg" as string]: `${seg}`,
                        ["--gap-off" as string]: `${offset}`,
                      } as React.CSSProperties)
                    : undefined
                }
              />
            );
          })
        )}
      </svg>

      {/* Terminal central glyph. */}
      {isTerminal &&
        (() => {
          const { color, Icon } = OUTCOME_META[
            outcome as Exclude<Outcome, "active">
          ];
          return (
            <Icon
              className="absolute inset-0 m-auto"
              style={{ width: size * 0.55, height: size * 0.55, color }}
              strokeWidth={3}
              aria-hidden
            />
          );
        })()}

      {/* Flag adornments (active specs only — terminal specs have no flags). */}
      {!isTerminal && blocked && (
        <Pause
          className="absolute -top-0.5 -right-0.5 text-amber-400"
          style={{ width: size * 0.45, height: size * 0.45 }}
          fill="currentColor"
          aria-label="Pausada"
        />
      )}
      {!isTerminal && waveFailed && (
        <AlertTriangle
          className="absolute -bottom-0.5 -right-0.5 text-red-400"
          style={{ width: size * 0.5, height: size * 0.5 }}
          aria-label="Onda falhou"
        />
      )}
    </span>
  );
}
