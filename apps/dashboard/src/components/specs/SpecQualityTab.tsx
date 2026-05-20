import { useState } from "react";
import { cn } from "@/lib/utils";
import { relativeTime } from "@/lib/time";
import type { SpecQualityItem } from "@/lib/types/specs";

interface SpecQualityTabProps {
  items: SpecQualityItem[];
}

const STATUS_CLS: Record<string, string> = {
  pass:    "bg-[--color-ok]/15 text-[--color-ok]",
  fail:    "bg-[--color-error]/15 text-[--color-error]",
  skip:    "bg-muted text-muted-foreground/60",
  unknown: "bg-muted text-muted-foreground/60",
};

const STATUS_LABEL: Record<string, string> = {
  pass:    "passou",
  fail:    "falhou",
  skip:    "pulado",
  unknown: "pendente",
};

function groupByWave(items: SpecQualityItem[]): [string, SpecQualityItem[]][] {
  const map = new Map<string, SpecQualityItem[]>();
  for (const item of items) {
    const key = item.wave != null ? `Onda ${item.wave}` : "Geral";
    const group = map.get(key) ?? [];
    group.push(item);
    map.set(key, group);
  }
  return Array.from(map.entries());
}

function AcRow({ item }: { item: SpecQualityItem }) {
  const [open, setOpen] = useState(false);
  const hasFail = !!item.fail_reason;

  return (
    <li className="flex flex-col gap-1 py-2 border-b border-border/40 last:border-b-0">
      <div className="flex items-start gap-2 flex-wrap">
        <span
          className={cn(
            "text-[10px] font-medium px-1.5 py-0.5 rounded uppercase tracking-wide shrink-0",
            STATUS_CLS[item.status] ?? STATUS_CLS.unknown,
          )}
        >
          {STATUS_LABEL[item.status] ?? item.status}
        </span>

        <span className="font-mono text-[12px] font-medium text-foreground/80 flex-1 min-w-0">
          {item.ac_label ?? item.ac_id}
        </span>

        {item.last_run_at && (
          <span className="text-[11px] text-muted-foreground/50 shrink-0 tabular-nums"
            style={{ fontVariantNumeric: "tabular-nums" }}
          >
            {relativeTime(item.last_run_at)}
          </span>
        )}
      </div>

      {item.command && (
        <code className="text-[11px] text-muted-foreground bg-muted/50 px-2 py-0.5 rounded font-mono block truncate"
          title={item.command}
        >
          {item.command}
        </code>
      )}

      {hasFail && (
        <button
          type="button"
          onClick={() => setOpen((o) => !o)}
          className="text-[11px] text-[--color-error]/80 hover:text-[--color-error] text-left transition-colors"
          aria-expanded={open}
        >
          {open ? "▾ ocultar motivo" : "▸ ver motivo"}
        </button>
      )}
      {open && hasFail && (
        <p className="text-[12px] text-muted-foreground leading-relaxed pl-2 border-l-2 border-[--color-error]/40">
          {item.fail_reason}
        </p>
      )}
    </li>
  );
}

export function SpecQualityTab({ items }: SpecQualityTabProps) {
  if (items.length === 0) {
    return (
      <p className="text-[13px] text-muted-foreground py-4 text-center">
        Nenhum critério de aceite registrado ainda.
      </p>
    );
  }

  const groups = groupByWave(items);

  return (
    <div className="flex flex-col gap-5">
      {groups.map(([wave, waveItems]) => (
        <section key={wave} className="flex flex-col gap-1">
          <h4 className="text-[10px] uppercase tracking-wide text-muted-foreground font-medium mb-1">
            {wave}
            <span className="text-muted-foreground/50 ml-1 tabular-nums"
              style={{ fontVariantNumeric: "tabular-nums" }}
            >
              {waveItems.filter((i) => i.status === "pass").length}/{waveItems.length}
            </span>
          </h4>
          <ul className="flex flex-col">
            {waveItems.map((item) => (
              <AcRow key={item.ac_id} item={item} />
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}
