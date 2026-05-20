/**
 * Middle-truncation: keep the first `start` chars, then …, then the last `end`
 * chars of the string. Preserves both the file name and enough path context.
 *
 * AC-13: never truncate at the start — always show the beginning of the path.
 */
export function midTruncate(s: string, start = 14, end = 10): string {
  if (s.length <= start + end + 1) return s;
  return `${s.slice(0, start)}…${s.slice(-end)}`;
}
