import { useQuery } from "@tanstack/react-query";
import {
  fetchPromptEconomy,
  fetchCollectorHealth,
  type PromptEconomy,
  type CollectorHealth,
} from "@/api/promptEconomy";

/**
 * Fetch the honest prompt-economy payload every 30s. `repoPath === null`
 * disables the query (e.g. when no workspace is selected).
 *
 * Mirrors the rest of the dashboard's TanStack Query usage: a stable
 * `queryKey` keyed on `repoPath`, `enabled` gating, and a 30s interval to
 * match the other Economia panels (see spec 2026-05-20-dashboard-ux-honest
 * Wave 3 — all economy hooks poll at the same cadence).
 */
export function usePromptEconomy(repoPath: string | null) {
  return useQuery<PromptEconomy>({
    queryKey: ["promptEconomy", repoPath],
    queryFn: () => fetchPromptEconomy(repoPath as string),
    enabled: !!repoPath,
    refetchInterval: 30_000,
  });
}

/**
 * Fetch the unified OTEL collector badge state. Every page consumes this hook
 * instead of deriving its own badge, so Telemetry and Prompt Economy always
 * show the same state at the same time. Refreshes fast (10s) because the badge
 * is the page's "is anything happening" signal.
 */
export function useCollectorHealth(repoPath: string | null) {
  return useQuery<CollectorHealth>({
    queryKey: ["collectorHealth", repoPath],
    queryFn: () => fetchCollectorHealth(repoPath as string),
    enabled: !!repoPath,
    refetchInterval: 10_000,
  });
}
