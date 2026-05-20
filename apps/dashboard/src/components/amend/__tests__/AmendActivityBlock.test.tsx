/**
 * AC-15: AmendActivityBlock
 *
 * Assertions:
 * - 3 synthetic pipeline.amend_* events for specId="X" are rendered
 * - Events appear in chronological order (oldest first)
 * - Correct icon and label per event kind
 *
 * Setup requirement: same as AmendMetricsCard.test.tsx
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { AmendActivityBlock } from "../AmendActivityBlock";

// ── mock Tauri invoke ────────────────────────────────────────────────────────

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

// ── mock zustand store ───────────────────────────────────────────────────────

vi.mock("@/lib/store", () => ({
  useStore: (selector: (s: Record<string, unknown>) => unknown) =>
    selector({ activeWorkspaceId: "proj-1", projectsRoot: "/projects" }),
}));

// ── synthetic amend events (out of order intentionally) ─────────────────────

const SYNTHETIC_EVENTS = [
  {
    event_type: "pipeline.amend_close",
    ts: "2026-05-20T12:03:00Z",
    spec: "X",
    summary: "window closed",
    wave: null,
    actor_id: null,
    actor_kind: null,
    tool_name: null,
    target: null,
    phase: null,
  },
  {
    event_type: "pipeline.amend_open",
    ts: "2026-05-20T12:00:00Z",
    spec: "X",
    summary: null,
    wave: null,
    actor_id: null,
    actor_kind: null,
    tool_name: null,
    target: null,
    phase: null,
  },
  {
    event_type: "pipeline.amend_capture",
    ts: "2026-05-20T12:01:30Z",
    spec: "X",
    summary: "edit detected",
    wave: null,
    actor_id: null,
    actor_kind: null,
    tool_name: null,
    target: null,
    phase: null,
  },
];

vi.mock("@/lib/dashboard", async (importOriginal) => {
  const mod = await importOriginal<typeof import("@/lib/dashboard")>();
  return {
    ...mod,
    useProjects: () => [{ id: "proj-1", name: "test", path: "/projects/test" }],
    fetchRecentEvents: vi.fn().mockResolvedValue(SYNTHETIC_EVENTS),
  };
});

function wrapper({ children }: { children: React.ReactNode }) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return <QueryClientProvider client={qc}>{children}</QueryClientProvider>;
}

// ── tests ─────────────────────────────────────────────────────────────────────

describe("AmendActivityBlock", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders all 3 amend events for specId=X", async () => {
    render(<AmendActivityBlock specId="X" />, { wrapper });

    await waitFor(() => {
      expect(screen.getByText(/janela aberta/i)).toBeInTheDocument();
    });

    expect(screen.getByText(/atividade capturada/i)).toBeInTheDocument();
    expect(screen.getByText(/janela fechada/i)).toBeInTheDocument();
  });

  it("renders events in chronological order (oldest first)", async () => {
    render(<AmendActivityBlock specId="X" />, { wrapper });

    await waitFor(() => {
      expect(screen.getByText(/janela aberta/i)).toBeInTheDocument();
    });

    const items = screen.getAllByRole("listitem");
    // First item: amend_open (earliest ts)
    expect(items[0]).toHaveTextContent(/janela aberta/i);
    // Second item: amend_capture
    expect(items[1]).toHaveTextContent(/atividade capturada/i);
    // Third item: amend_close (latest ts)
    expect(items[2]).toHaveTextContent(/janela fechada/i);
  });

  it("renders correct icons for each event kind", async () => {
    render(<AmendActivityBlock specId="X" />, { wrapper });

    await waitFor(() => {
      expect(screen.getByText("○")).toBeInTheDocument(); // amend_open
    });

    expect(screen.getByText("✎")).toBeInTheDocument();  // amend_capture
    expect(screen.getByText("✓")).toBeInTheDocument();  // amend_close
  });

  it("returns null (renders nothing) when specId has no amend events", async () => {
    const { fetchRecentEvents } = await import("@/lib/dashboard");
    vi.mocked(fetchRecentEvents).mockResolvedValue([]);

    const { container } = render(<AmendActivityBlock specId="NO_EVENTS" />, { wrapper });

    // After settling, no content rendered
    await waitFor(() => {
      expect(container.firstChild).toBeNull();
    });
  });
});
