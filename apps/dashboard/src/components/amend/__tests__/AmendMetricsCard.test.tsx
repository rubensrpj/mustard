/**
 * AC-16: AmendMetricsCard
 *
 * Assertions:
 * - 4 metric tiles render with mocked values
 * - Empty state appears when all values are zero/empty
 *
 * Setup requirement: add vitest + @testing-library/react + jsdom to devDeps
 * before running. The test file is structurally correct — the runner is not
 * yet wired in package.json (pnpm add -D vitest @testing-library/react jsdom
 * @vitejs/plugin-react @testing-library/jest-dom).
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { AmendMetricsCard } from "../AmendMetricsCard";

// ── mock @tauri-apps/api/core (invoke is used via dashboard.ts wrappers) ────

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

// ── mock zustand store ───────────────────────────────────────────────────────

vi.mock("@/lib/store", () => ({
  useStore: (selector: (s: Record<string, unknown>) => unknown) =>
    selector({ activeWorkspaceId: "proj-1", projectsRoot: "/projects" }),
}));

// ── mock dashboard helpers that depend on Tauri ──────────────────────────────

vi.mock("@/lib/dashboard", async (importOriginal) => {
  const mod = await importOriginal<typeof import("@/lib/dashboard")>();
  return {
    ...mod,
    useProjects: () => [{ id: "proj-1", name: "test", path: "/projects/test" }],
    fetchAmendResolutionRate: vi.fn().mockResolvedValue(0.75),
    fetchAmendDriftRate: vi.fn().mockResolvedValue(0.1),
    fetchCrossSessionAmendCount: vi.fn().mockResolvedValue(3),
    fetchAmendWindowDuration: vi.fn().mockResolvedValue([1200, 3400, 900]),
  };
});

function wrapper({ children }: { children: React.ReactNode }) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return <QueryClientProvider client={qc}>{children}</QueryClientProvider>;
}

// ── tests ─────────────────────────────────────────────────────────────────────

describe("AmendMetricsCard", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders 4 metric tiles with mocked values", async () => {
    render(<AmendMetricsCard />, { wrapper });

    // Wait for async queries to resolve
    await waitFor(() => {
      expect(screen.getByText(/taxa de resolução/i)).toBeInTheDocument();
    });

    // Resolution rate: 0.75 → 75.0%
    expect(screen.getByText("75.0%")).toBeInTheDocument();

    // Drift rate: 0.1 → 10.0%
    expect(screen.getByText("10.0%")).toBeInTheDocument();

    // Cross-session pending count
    expect(screen.getByText("3")).toBeInTheDocument();

    // Average duration: avg([1200, 3400, 900]) = 1833ms → "1.8 s"
    expect(screen.getByText(/1\.[0-9] s/)).toBeInTheDocument();
  });

  it("shows empty state when all values are zero", async () => {
    const { fetchAmendResolutionRate, fetchAmendDriftRate, fetchCrossSessionAmendCount, fetchAmendWindowDuration } =
      await import("@/lib/dashboard");

    vi.mocked(fetchAmendResolutionRate).mockResolvedValue(0);
    vi.mocked(fetchAmendDriftRate).mockResolvedValue(0);
    vi.mocked(fetchCrossSessionAmendCount).mockResolvedValue(0);
    vi.mocked(fetchAmendWindowDuration).mockResolvedValue([]);

    render(<AmendMetricsCard />, { wrapper });

    await waitFor(() => {
      expect(
        screen.getByText(/nenhuma janela amend ainda registrada/i),
      ).toBeInTheDocument();
    });
  });
});
