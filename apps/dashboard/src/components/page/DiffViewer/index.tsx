// LCS algorithm adapted from claude-devtools (MIT). See NOTICE.md.
//
// Pure-presentational diff viewer. No `diff`, `react-diff`, `diff2html`,
// `jsdiff` or similar — we run a classic O(m*n) Longest Common Subsequence
// over the two line arrays and emit `+` / `-` / ` ` lines. This is fine for
// the trace-viewer and Economia-page use cases (typically <1k lines per
// side) and keeps the bundle weightless.

import { useMemo } from "react";
import { cn } from "@/lib/utils";

export type DiffMode = "unified" | "split";

export interface DiffViewerProps {
  before: string;
  after: string;
  mode?: DiffMode;
  maxLines?: number;
  className?: string;
}

type Op = "equal" | "add" | "del";

interface DiffLine {
  op: Op;
  text: string;
  /** 1-indexed source line number; null when the side has no corresponding line. */
  beforeNo: number | null;
  afterNo: number | null;
}

/** Build the LCS length table for `a` vs `b`. Returns a (m+1)x(n+1) matrix. */
function lcsTable(a: string[], b: string[]): Uint32Array {
  const m = a.length;
  const n = b.length;
  const t = new Uint32Array((m + 1) * (n + 1));
  for (let i = m - 1; i >= 0; i--) {
    for (let j = n - 1; j >= 0; j--) {
      const idx = i * (n + 1) + j;
      if (a[i] === b[j]) {
        t[idx] = t[(i + 1) * (n + 1) + (j + 1)] + 1;
      } else {
        const down = t[(i + 1) * (n + 1) + j];
        const right = t[i * (n + 1) + (j + 1)];
        t[idx] = down > right ? down : right;
      }
    }
  }
  return t;
}

/** Walk the LCS table to produce an ordered list of diff lines. */
function diffLines(beforeText: string, afterText: string): DiffLine[] {
  const a = beforeText.split("\n");
  const b = afterText.split("\n");
  const n = b.length;
  const t = lcsTable(a, b);
  const out: DiffLine[] = [];
  let i = 0;
  let j = 0;
  let aNo = 1;
  let bNo = 1;
  while (i < a.length && j < b.length) {
    if (a[i] === b[j]) {
      out.push({ op: "equal", text: a[i], beforeNo: aNo, afterNo: bNo });
      i++; j++; aNo++; bNo++;
    } else if (t[(i + 1) * (n + 1) + j] >= t[i * (n + 1) + (j + 1)]) {
      out.push({ op: "del", text: a[i], beforeNo: aNo, afterNo: null });
      i++; aNo++;
    } else {
      out.push({ op: "add", text: b[j], beforeNo: null, afterNo: bNo });
      j++; bNo++;
    }
  }
  while (i < a.length) {
    out.push({ op: "del", text: a[i], beforeNo: aNo, afterNo: null });
    i++; aNo++;
  }
  while (j < b.length) {
    out.push({ op: "add", text: b[j], beforeNo: null, afterNo: bNo });
    j++; bNo++;
  }
  return out;
}

const ROW = "grid grid-cols-[3rem_3rem_1rem_1fr] gap-2 font-mono text-[12px] leading-[1.55] px-2";
// TF remap: --ds-text-tertiary → --muted-foreground; no tertiary tier in Binance
const GUTTER = "text-right text-[--muted-foreground] select-none";

function Sigil({ op }: { op: Op }) {
  const ch = op === "add" ? "+" : op === "del" ? "-" : " ";
  // TF remap: --ds-text-tertiary → --muted-foreground
  return <span className="text-[--muted-foreground] select-none">{ch}</span>;
}

function bgFor(op: Op): string {
  // TF remap: --ds-intent-success → --intent-success; --ds-intent-error → --intent-error; --ds-text-primary → --foreground; --ds-text-secondary → --muted-foreground
  if (op === "add") return "bg-[--intent-success]/10 text-[--foreground]";
  if (op === "del") return "bg-[--intent-error]/10 text-[--foreground]";
  return "text-[--muted-foreground]";
}

export function DiffViewer({
  before,
  after,
  mode = "unified",
  maxLines,
  className,
}: DiffViewerProps) {
  const lines = useMemo(() => diffLines(before, after), [before, after]);
  const shown = typeof maxLines === "number" ? lines.slice(0, maxLines) : lines;
  const truncated = shown.length < lines.length;

  if (mode === "split") {
    return (
      <div
        className={cn(
          // TF remap: --ds-radius-md → var(--radius-card); --ds-surface-hover → --accent; --ds-surface-sunken → --background
          "rounded-[--radius-card] border border-[--accent] bg-[--background] overflow-hidden",
          className,
        )}
      >
        {/* TF remap: --ds-surface-hover → --accent */}
        <div className="grid grid-cols-2 divide-x divide-[--accent]">
          <div>
            {shown.map((l, idx) => (
              <div
                key={`L${idx}`}
                className={cn("grid grid-cols-[3rem_1rem_1fr] gap-2 font-mono text-[12px] leading-[1.55] px-2", l.op === "del" ? bgFor("del") : l.op === "add" ? "" : bgFor("equal"))}
              >
                <span className={GUTTER}>{l.beforeNo ?? ""}</span>
                <Sigil op={l.op === "add" ? "equal" : l.op} />
                <span className="whitespace-pre-wrap break-words">{l.op === "add" ? "" : l.text}</span>
              </div>
            ))}
          </div>
          <div>
            {shown.map((l, idx) => (
              <div
                key={`R${idx}`}
                className={cn("grid grid-cols-[3rem_1rem_1fr] gap-2 font-mono text-[12px] leading-[1.55] px-2", l.op === "add" ? bgFor("add") : l.op === "del" ? "" : bgFor("equal"))}
              >
                <span className={GUTTER}>{l.afterNo ?? ""}</span>
                <Sigil op={l.op === "del" ? "equal" : l.op} />
                <span className="whitespace-pre-wrap break-words">{l.op === "del" ? "" : l.text}</span>
              </div>
            ))}
          </div>
        </div>
        {truncated && (
          // TF remap: --ds-text-tertiary → --muted-foreground; --ds-surface-hover → --accent
          <div className="px-3 py-1.5 text-[11px] text-[--muted-foreground] border-t border-[--accent]">… {lines.length - shown.length} more lines</div>
        )}
      </div>
    );
  }

  return (
    <div
      className={cn(
        // TF remap: --ds-radius-md → var(--radius-card); --ds-surface-hover → --accent; --ds-surface-sunken → --background
        "rounded-[--radius-card] border border-[--accent] bg-[--background] overflow-hidden",
        className,
      )}
    >
      {shown.map((l, idx) => (
        <div key={idx} className={cn(ROW, bgFor(l.op))}>
          <span className={GUTTER}>{l.beforeNo ?? ""}</span>
          <span className={GUTTER}>{l.afterNo ?? ""}</span>
          <Sigil op={l.op} />
          <span className="whitespace-pre-wrap break-words">{l.text}</span>
        </div>
      ))}
      {truncated && (
        // TF remap: --ds-text-tertiary → --muted-foreground; --ds-surface-hover → --accent
        <div className="px-3 py-1.5 text-[11px] text-[--muted-foreground] border-t border-[--accent]">… {lines.length - shown.length} more lines</div>
      )}
    </div>
  );
}
