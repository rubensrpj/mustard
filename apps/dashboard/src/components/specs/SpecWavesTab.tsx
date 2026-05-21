import { cn } from "@/lib/utils";
import { relativeTime } from "@/lib/time";
import type { SpecWave } from "@/lib/types/specs";

interface SpecWavesTabProps {
  waves: SpecWave[];
}

/**
 * Wave status palette — mirrors `mustard_specsdb::WaveStatus`.
 * Mustard yellow for the active wave, --color-ok for completed,
 * --color-error for failed, neutral grey for queued. AC-12 of
 * spec 2026-05-20-dashboard-ux-honest pins this file as the one
 * that must read wave.status + render formatDuration.
 */
const STATUS_CLS: Record<string, string> = {
  completed:   "bg-[--color-ok]/15 text-[--color-ok]",
  in_progress: "bg-[--color-accent-mustard]/15 text-[--color-accent-mustard]",
  failed:      "bg-[--color-error]/15 text-[--color-error]",
  queued:      "bg-muted text-muted-foreground",
};

const STATUS_LABEL: Record<string, string> = {
  completed:   "concluída",
  in_progress: "em execução",
  failed:      "falhou",
  queued:      "aguardando",
};

/** Format milliseconds into a compact "1h 2m" / "12s" string. */
function formatDuration(ms: number | null): string {
  if (ms == null || ms <= 0) return "—";
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  const sec = s % 60;
  if (m < 60) return sec > 0 ? `${m}m ${sec}s` : `${m}m`;
  const h = Math.floor(m / 60);
  const min = m % 60;
  return min > 0 ? `${h}h ${min}m` : `${h}h`;
}

/** Compute duration_ms from started_at/completed_at when present. */
function waveDurationMs(wave: SpecWave): number | null {
  if (!wave.started_at) return null;
  const start = new Date(wave.started_at).getTime();
  const end = wave.completed_at
    ? new Date(wave.completed_at).getTime()
    : wave.status === "in_progress"
      ? Date.now()
      : null;
  if (end == null) return null;
  const diff = end - start;
  return Number.isFinite(diff) && diff >= 0 ? diff : null;
}

export function SpecWavesTab({ waves }: SpecWavesTabProps) {
  if (waves.length === 0) {
    return (
      <p className="text-[13px] text-muted-foreground py-4 text-center">
        Nenhuma onda registrada para esta spec.
      </p>
    );
  }

  return (
    <ul className="flex flex-col gap-2">
      {waves.map((wave) => {
        const duration_ms = waveDurationMs(wave);
        const isFailed = wave.status === "failed";
        const borderClass =
          wave.status === "completed"
            ? "border-[--color-ok]/30"
            : wave.status === "in_progress"
              ? "border-[--color-accent-mustard]/40"
              : isFailed
                ? "border-[--color-error]/40"
                : "border-border/50";

        return (
          <li
            key={wave.wave}
            className={cn(
              "flex flex-col gap-1.5 px-3 py-2.5 rounded-md border bg-card/10",
              borderClass,
            )}
          >
            <div className="flex items-start gap-3">
              {/* Wave number */}
              <span
                className="text-[12px] font-mono font-medium text-muted-foreground shrink-0 tabular-nums pt-0.5"
                style={{ fontVariantNumeric: "tabular-nums" }}
              >
                #{wave.wave}
              </span>

              {/* Role + agent */}
              <div className="flex-1 min-w-0 flex flex-col gap-0.5">
                <div className="flex items-center gap-2 flex-wrap">
                  {wave.role && (
                    <span className="text-[12px] font-medium text-foreground/80 truncate">
                      {wave.role}
                    </span>
                  )}
                  {wave.agent_type && (
                    <span className="text-[11px] font-mono text-muted-foreground/70 bg-muted px-1 rounded">
                      {wave.agent_type}
                    </span>
                  )}
                </div>

                <div className="flex items-center gap-3 text-[11px] text-muted-foreground flex-wrap tabular-nums"
                  style={{ fontVariantNumeric: "tabular-nums" }}
                >
                  {wave.started_at && (
                    <span>início: {relativeTime(wave.started_at)}</span>
                  )}
                  {wave.completed_at && (
                    <span>fim: {relativeTime(wave.completed_at)}</span>
                  )}
                  {duration_ms != null && (
                    <span title="duração total da onda">
                      duração: {formatDuration(duration_ms)}
                    </span>
                  )}
                  <span>
                    {wave.files_changed}{" "}
                    {wave.files_changed === 1 ? "arquivo" : "arquivos"}
                  </span>
                </div>
              </div>

              {/* Status pill — driven by wave.status */}
              <span
                className={cn(
                  "text-[10px] font-medium px-1.5 py-0.5 rounded uppercase tracking-wide shrink-0",
                  STATUS_CLS[wave.status] ?? "bg-muted text-muted-foreground",
                )}
              >
                {STATUS_LABEL[wave.status] ?? wave.status}
              </span>
            </div>

            {/* Last-error preview when the wave failed. The shape currently
                doesn't carry a structured error blob, so the "ver detalhes"
                hint nudges the user to open the markdown viewer or QA tab
                where the actual stderr lives. */}
            {isFailed && (
              <p className="text-[11px] text-[--color-error]/80 pl-7">
                Onda falhou — ver Qualidade / markdown para detalhes do último erro.
              </p>
            )}
          </li>
        );
      })}
    </ul>
  );
}
