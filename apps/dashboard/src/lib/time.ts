import dayjs from "dayjs";
import relativeTimePlugin from "dayjs/plugin/relativeTime";
import "dayjs/locale/pt-br";
import type { EconomyWindowPeriod, TimeWindow } from "@/lib/types/economy";

dayjs.extend(relativeTimePlugin);
dayjs.locale("pt-br");

export function relativeTime(iso: string | null | undefined): string {
  if (!iso) return "";
  try {
    const d = dayjs(iso);
    if (!d.isValid()) return iso;
    return d.fromNow();
  } catch {
    return iso ?? "";
  }
}

/** Days covered by each Economia window period — single source for the
 *  period → span mapping shared by the selector and `economyWindow`. */
const WINDOW_DAYS: Record<EconomyWindowPeriod, number> = {
  "1d": 1,
  "7d": 7,
  "15d": 15,
  "30d": 30,
};

/**
 * Derive the concrete `TimeWindow` for an Economia period: `from` is `now - N
 * days` as ISO-8601 (the shape core `TimeWindow` deserializes), `to` is left
 * unbounded (omitted) so the window runs up to the present.
 *
 * Reads the CURRENT time at call, so the returned `from` moves between calls —
 * invoke it at fetch time (inside the `queryFn`) and key the query on the
 * stable period, never on this bound, or the cache would churn every render.
 */
export function economyWindow(period: EconomyWindowPeriod): TimeWindow {
  return { from: dayjs().subtract(WINDOW_DAYS[period], "day").toISOString() };
}
