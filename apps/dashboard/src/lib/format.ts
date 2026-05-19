export function formatNumber(n: number): string {
  if (!Number.isFinite(n)) return "0";
  const abs = Math.abs(n);
  if (abs < 1_000) return String(Math.round(n));
  if (abs < 1_000_000) return `${(n / 1_000).toFixed(1).replace(/\.0$/, "")}K`;
  if (abs < 1_000_000_000) return `${(n / 1_000_000).toFixed(1).replace(/\.0$/, "")}M`;
  return `${(n / 1_000_000_000).toFixed(1).replace(/\.0$/, "")}G`;
}

export function formatTokens(n: number): string {
  return formatNumber(n);
}

export function formatPct(p: number): string {
  if (!Number.isFinite(p)) return "0%";
  return `${p.toFixed(1)}%`;
}

export function formatUsd(n: number): string {
  if (!Number.isFinite(n) || n === 0) return "$0";
  const abs = Math.abs(n);
  if (abs < 0.01) return "<$0.01";
  if (abs < 1_000) return `$${n.toFixed(2)}`;
  if (abs < 1_000_000) return `$${(n / 1_000).toFixed(2)}K`;
  return `$${(n / 1_000_000).toFixed(2)}M`;
}

export function formatDurationMs(ms: number): string {
  if (ms < 1_000) return "<1s";
  if (ms < 60_000) return `${Math.round(ms / 1_000)}s`;
  if (ms < 3_600_000) return `${Math.floor(ms / 60_000)}m ${Math.round((ms % 60_000) / 1_000)}s`;
  return `${Math.floor(ms / 3_600_000)}h ${Math.round((ms % 3_600_000) / 60_000)}m`;
}
