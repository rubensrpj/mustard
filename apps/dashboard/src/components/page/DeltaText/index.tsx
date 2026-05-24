// Inline delta indicator (e.g. "+12.4%" in green, "-3.0%" in red). The
// `intent` prop defaults to "auto" which derives the color from the sign
// of `value` — positive maps to success, negative to error, zero to
// neutral. Callers can force a specific intent when the semantics flip
// (e.g. a cost delta where "+5%" is bad and should render red).

import { cn } from "@/lib/utils";

export type DeltaFormat = "pct" | "abs";
export type DeltaIntent = "auto" | "success" | "error" | "neutral";

export interface DeltaTextProps {
  value: number;
  format?: DeltaFormat;
  intent?: DeltaIntent;
  className?: string;
}

function resolveIntent(value: number, intent: DeltaIntent): Exclude<DeltaIntent, "auto"> {
  if (intent !== "auto") return intent;
  if (value > 0) return "success";
  if (value < 0) return "error";
  return "neutral";
}

const INTENT_TEXT: Record<Exclude<DeltaIntent, "auto">, string> = {
  success: "text-[--intent-success]",
  error: "text-[--intent-error]",
  neutral: "text-muted-foreground",
};

function formatDelta(value: number, format: DeltaFormat): string {
  const sign = value > 0 ? "+" : value < 0 ? "-" : "";
  const abs = Math.abs(value);
  if (format === "pct") {
    return `${sign}${abs.toFixed(1)}%`;
  }
  return `${sign}${abs}`;
}

export function DeltaText({
  value,
  format = "pct",
  intent = "auto",
  className,
}: DeltaTextProps) {
  const resolved = resolveIntent(value, intent);
  return (
    <span
      className={cn(
        "font-mono tabular-nums text-[12px] leading-none",
        INTENT_TEXT[resolved],
        className,
      )}
    >
      {formatDelta(value, format)}
    </span>
  );
}
