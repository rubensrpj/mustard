import type { ReactNode } from "react";
import { cn } from "@/lib/utils";
import { waveNumber, waveRole, waveState, waveStateLabel, type WaveState } from "@/lib/waves";
import type { SpecRow } from "@/lib/dashboard";

type WaveNavProps = {
  parentName: string;
  waves: SpecRow[];
  /** Spec name currently shown in the panel body. */
  current: string;
  onSelect: (specName: string) => void;
};

/**
 * Navegador de wave plan — uma faixa de chips com o plano-mãe e cada onda.
 * Aparece no painel de detalhe quando a spec é decomposta em waves, pra
 * alternar entre a spec pai e o `spec.md` de cada onda sem sair do painel.
 * Cada chip de wave traz um ponto de estado: concluída, em andamento ou
 * pendente.
 */
export function WaveNav({ parentName, waves, current, onSelect }: WaveNavProps) {
  return (
    <div className="sticky top-0 z-10 flex flex-col gap-1.5 px-5 py-2.5 border-b border-border bg-background">
      <div className="flex items-center gap-1.5 flex-wrap">
        <span className="text-[10px] uppercase tracking-wider text-muted-foreground/60 mr-1">
          Waves
        </span>
        <Chip selected={current === parentName} onClick={() => onSelect(parentName)}>
          Plano
        </Chip>
        {waves.map((w) => {
          const n = waveNumber(w.name);
          const state = waveState(w);
          return (
            <Chip
              key={w.name}
              selected={current === w.name}
              state={state}
              onClick={() => onSelect(w.name)}
              title={`${w.name} · ${waveRole(w.name)} · ${waveStateLabel(state)}`}
            >
              {n !== null ? `W${n}` : w.name}
            </Chip>
          );
        })}
      </div>
      <WaveLegend />
    </div>
  );
}

function WaveLegend() {
  const states: WaveState[] = ["done", "active", "pending"];
  return (
    <div className="flex items-center gap-3">
      {states.map((st) => (
        <span
          key={st}
          className="flex items-center gap-1 text-[10px] text-muted-foreground/55"
        >
          <StateDot state={st} />
          {waveStateLabel(st)}
        </span>
      ))}
    </div>
  );
}

function StateDot({ state }: { state: WaveState }) {
  return (
    <span
      aria-hidden
      className={cn(
        "inline-block size-1.5 rounded-full shrink-0",
        state === "done" && "bg-[--intent-success]",
        state === "active" && "bg-[--primary] animate-pulse",
        state === "pending" && "bg-zinc-500/60",
        state === "cancelled" && "bg-[--intent-error]",
      )}
    />
  );
}

function Chip({
  selected,
  state,
  onClick,
  title,
  children,
}: {
  selected: boolean;
  state?: WaveState;
  onClick: () => void;
  title?: string;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      className={cn(
        "inline-flex items-center gap-1.5 rounded-md px-2 py-0.5 text-[11px] font-medium border transition-colors tabular-nums",
        selected
          ? "bg-primary/20 text-primary border-primary/40"
          : "bg-transparent text-muted-foreground border-border hover:bg-muted/40 hover:text-foreground",
        state === "cancelled" && !selected && "line-through opacity-70",
      )}
    >
      {state && <StateDot state={state} />}
      {children}
    </button>
  );
}
