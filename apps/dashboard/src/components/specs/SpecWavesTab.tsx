import { cn } from "@/lib/utils";
import { relativeTime } from "@/lib/time";
import type { SpecWave } from "@/lib/types/specs";

interface SpecWavesTabProps {
  waves: SpecWave[];
}

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
      {waves.map((w) => (
        <li
          key={w.wave}
          className="flex items-start gap-3 px-3 py-2.5 rounded-md border border-border/50 bg-card/10"
        >
          {/* Wave number */}
          <span
            className="text-[12px] font-mono font-medium text-muted-foreground shrink-0 tabular-nums pt-0.5"
            style={{ fontVariantNumeric: "tabular-nums" }}
          >
            #{w.wave}
          </span>

          {/* Role + agent */}
          <div className="flex-1 min-w-0 flex flex-col gap-0.5">
            <div className="flex items-center gap-2 flex-wrap">
              {w.role && (
                <span className="text-[12px] font-medium text-foreground/80 truncate">
                  {w.role}
                </span>
              )}
              {w.agent_type && (
                <span className="text-[11px] font-mono text-muted-foreground/70 bg-muted px-1 rounded">
                  {w.agent_type}
                </span>
              )}
            </div>

            <div className="flex items-center gap-3 text-[11px] text-muted-foreground flex-wrap tabular-nums"
              style={{ fontVariantNumeric: "tabular-nums" }}
            >
              {w.started_at && (
                <span>início: {relativeTime(w.started_at)}</span>
              )}
              {w.completed_at && (
                <span>fim: {relativeTime(w.completed_at)}</span>
              )}
              <span>
                {w.files_changed}{" "}
                {w.files_changed === 1 ? "arquivo" : "arquivos"}
              </span>
            </div>
          </div>

          {/* Status pill */}
          <span
            className={cn(
              "text-[10px] font-medium px-1.5 py-0.5 rounded uppercase tracking-wide shrink-0",
              STATUS_CLS[w.status] ?? "bg-muted text-muted-foreground",
            )}
          >
            {STATUS_LABEL[w.status] ?? w.status}
          </span>
        </li>
      ))}
    </ul>
  );
}
