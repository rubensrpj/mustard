/**
 * ChangeRequestActivityBlock tests.
 *
 * Uses the dashboard's standard activity-block mock style. Assertions:
 * - pipeline.change.request events for specId="X" render (with their summary)
 * - chronological order (oldest first)
 * - returns null (renders nothing) when the spec has no change requests
 * - events from another spec are ignored
 *
 * NOTE: vitest is not installed in this package and `__tests__/` is excluded
 * from `tsc -b`; this file mirrors the amend test for parity / future wiring.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ChangeRequestActivityBlock } from "../ChangeRequestActivityBlock";

// ── mock Tauri invoke ────────────────────────────────────────────────────────

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

// ── mock zustand store ───────────────────────────────────────────────────────

vi.mock("@/lib/store", () => ({
  useStore: (selector: (s: Record<string, unknown>) => unknown) =>
    selector({ activeWorkspaceId: "proj-1", projectsRoot: "/projects" }),
}));

// ── synthetic change-request events (out of order + a foreign-spec event) ────

const SYNTHETIC_EVENTS = [
  {
    event_type: "pipeline.change.request",
    ts: "2026-05-20T12:03:00Z",
    spec: "X",
    summary: "solicitação (Execute) — segundo pedido",
    wave: null,
    actor_id: null,
    actor_kind: null,
    tool_name: null,
    target: null,
    phase: null,
  },
  {
    event_type: "pipeline.change.request",
    ts: "2026-05-20T12:00:00Z",
    spec: "X",
    summary: "solicitação (Plan) — primeiro pedido",
    wave: null,
    actor_id: null,
    actor_kind: null,
    tool_name: null,
    target: null,
    phase: null,
  },
  {
    // Belongs to a DIFFERENT spec — must be ignored for specId="X".
    event_type: "pipeline.change.request",
    ts: "2026-05-20T12:02:00Z",
    spec: "OTHER",
    summary: "solicitação (Execute) — spec alheia",
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

describe("ChangeRequestActivityBlock", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders the change-request events for specId=X (by summary)", async () => {
    render(<ChangeRequestActivityBlock specId="X" />, { wrapper });

    await waitFor(() => {
      expect(screen.getByText(/primeiro pedido/i)).toBeInTheDocument();
    });

    expect(screen.getByText(/segundo pedido/i)).toBeInTheDocument();
    // The header is present.
    expect(screen.getByText(/solicitações/i)).toBeInTheDocument();
  });

  it("renders events in chronological order (oldest first)", async () => {
    render(<ChangeRequestActivityBlock specId="X" />, { wrapper });

    await waitFor(() => {
      expect(screen.getByText(/primeiro pedido/i)).toBeInTheDocument();
    });

    const items = screen.getAllByRole("listitem");
    expect(items).toHaveLength(2);
    expect(items[0]).toHaveTextContent(/primeiro pedido/i); // earliest ts
    expect(items[1]).toHaveTextContent(/segundo pedido/i); // latest ts
  });

  it("ignores change-request events that belong to another spec", async () => {
    render(<ChangeRequestActivityBlock specId="X" />, { wrapper });

    await waitFor(() => {
      expect(screen.getByText(/primeiro pedido/i)).toBeInTheDocument();
    });

    // The OTHER-spec request must NOT leak into the X block.
    expect(screen.queryByText(/spec alheia/i)).not.toBeInTheDocument();
    expect(screen.getAllByRole("listitem")).toHaveLength(2);
  });

  it("returns null (renders nothing) when specId has no change requests", async () => {
    const { fetchRecentEvents } = await import("@/lib/dashboard");
    vi.mocked(fetchRecentEvents).mockResolvedValue([]);

    const { container } = render(
      <ChangeRequestActivityBlock specId="NO_EVENTS" />,
      { wrapper },
    );

    // After settling, no content rendered.
    await waitFor(() => {
      expect(container.firstChild).toBeNull();
    });
  });
});
