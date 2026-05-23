import { useEffect, useMemo, useRef, useState } from "react";
import { useSpecChildren } from "@/hooks/useSpecChildren";
import { useSpecWaves } from "@/hooks/useSpecWaves";
import { useSpecMemoryCrossWave } from "@/hooks/useSpecMemoryCrossWave";
import { EmptyState } from "@/components/page";
import { Markdown } from "@/components/Markdown";
import {
  radialLayout,
  simulate,
  type GraphEdge,
  type GraphNode,
  type NodeKind,
} from "./spec-graph-layout";

interface SpecNetworkTabProps {
  /** Absolute path to the active project repo. Backend resolves the spec
   *  directory under `<repoPath>/.claude/spec/{name}/`. */
  repoPath: string | null;
  /** Spec name as stored under `.claude/spec/{name}/`. */
  specName: string;
}

const PARENT_ID_PREFIX = "parent:";
const WAVE_ID_PREFIX = "wave:";
const SUBSPEC_ID_PREFIX = "sub:";
// Fallback canvas size used before the ResizeObserver delivers a real one.
const FALLBACK_W = 800;
const FALLBACK_H = 600;

// `simulate` is kept exported from the layout module for back-compat with any
// downstream test or consumer. Reference it once so unused-import lint stays
// quiet without forcing callers to know about the rename.
void simulate;

interface BuildResult {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

interface BuildInput {
  specName: string;
  waves: { wave: number; role: string | null }[];
  children: { spec: string }[];
}

/** Build nodes + edges from sub-spec list and wave list. Pure helper so it
 *  memoizes cleanly under `useMemo`. */
function buildGraph({ specName, waves, children }: BuildInput): BuildResult {
  const nodes: GraphNode[] = [];
  const edges: GraphEdge[] = [];

  const parentId = `${PARENT_ID_PREFIX}${specName}`;
  nodes.push({ id: parentId, label: specName, kind: "parent", x: 0, y: 0 });

  // Waves sorted by number — predictable radial placement.
  const sortedWaves = [...waves].sort((a, b) => a.wave - b.wave);
  const waveIds = new Map<number, string>();
  for (const w of sortedWaves) {
    const id = `${WAVE_ID_PREFIX}${w.wave}`;
    const label = w.role ? `wave-${w.wave}-${w.role}` : `wave-${w.wave}`;
    nodes.push({ id, label, kind: "wave", x: 0, y: 0 });
    waveIds.set(w.wave, id);
    edges.push({ from: parentId, to: id, kind: "parent-child" });
  }

  // Wave-chain edges: wave-(N-1) → wave-N when both exist.
  for (let i = 1; i < sortedWaves.length; i++) {
    const prev = waveIds.get(sortedWaves[i - 1].wave);
    const curr = waveIds.get(sortedWaves[i].wave);
    if (prev && curr) edges.push({ from: prev, to: curr, kind: "wave-chain" });
  }

  for (const c of children) {
    const id = `${SUBSPEC_ID_PREFIX}${c.spec}`;
    nodes.push({ id, label: c.spec, kind: "subspec", x: 0, y: 0 });
    edges.push({ from: parentId, to: id, kind: "spec-link" });
  }

  return { nodes, edges };
}

function colorFor(kind: NodeKind): string {
  switch (kind) {
    case "parent":
      return "var(--color-accent-mustard)";
    case "wave":
      return "#5b8def";
    case "subspec":
      return "var(--color-muted-foreground)";
  }
}

function radiusFor(kind: NodeKind, hovered: boolean): number {
  const base = kind === "parent" ? 10 : kind === "wave" ? 7 : 5;
  return hovered ? base + 2 : base;
}

function labelSizeFor(kind: NodeKind): string {
  switch (kind) {
    case "parent":
      return "text-[13px] font-medium";
    case "wave":
      return "text-[12px]";
    case "subspec":
      return "text-[10px]";
  }
}

/**
 * Wave-3 (2026-05-21, spec `2026-05-21-dashboard-spec-tabs-polish`) — Rede tab
 * rebuilt as a deterministic radial mind-map. Parent sits at the canvas
 * center, waves orbit on an outer ring, sub-specs cluster in an arc around
 * their wave. SVG fills its container (no aspect ratio); a `ResizeObserver`
 * drives re-layout when the panel resizes.
 */
export function SpecNetworkTab({ repoPath, specName }: SpecNetworkTabProps) {
  const childrenQ = useSpecChildren(repoPath, specName);
  const wavesQ = useSpecWaves(repoPath, specName);

  const { nodes, edges } = useMemo<BuildResult>(() => {
    return buildGraph({
      specName,
      waves: wavesQ.data ?? [],
      children: childrenQ.data ?? [],
    });
  }, [specName, wavesQ.data, childrenQ.data]);

  // Track the panel size so the radial layout fills whatever space the
  // sidebar leaves us. Starts at the fallback constants and updates via a
  // ResizeObserver attached to `canvasRef` below.
  const canvasRef = useRef<HTMLDivElement | null>(null);
  const [size, setSize] = useState<{ w: number; h: number }>({
    w: FALLBACK_W,
    h: FALLBACK_H,
  });

  useEffect(() => {
    const node = canvasRef.current;
    if (!node) return;
    const ro = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const w = Math.max(1, Math.floor(entry.contentRect.width));
        const h = Math.max(1, Math.floor(entry.contentRect.height));
        setSize((curr) => (curr.w === w && curr.h === h ? curr : { w, h }));
      }
    });
    ro.observe(node);
    return () => ro.disconnect();
  }, []);

  const placed = useMemo(() => {
    return radialLayout(nodes, edges, { width: size.w, height: size.h });
  }, [nodes, edges, size]);

  // Adjacency: per-node neighbor set for hover fading.
  const neighbors = useMemo(() => {
    const m = new Map<string, Set<string>>();
    for (const n of placed) m.set(n.id, new Set<string>());
    for (const e of edges) {
      m.get(e.from)?.add(e.to);
      m.get(e.to)?.add(e.from);
    }
    return m;
  }, [placed, edges]);

  const placedById = useMemo(() => {
    const m = new Map<string, GraphNode>();
    for (const n of placed) m.set(n.id, n);
    return m;
  }, [placed]);

  const [hoveredId, setHoveredId] = useState<string | null>(null);

  // Current wave: highest wave number + 1 (the "next" wave whose priors we
  // surface). If no waves yet, hold null and skip the memory query.
  const totalWaves = (wavesQ.data ?? []).length;
  const currentWave = totalWaves > 0 ? totalWaves + 1 : null;

  const memoryQ = useSpecMemoryCrossWave(repoPath, specName, currentWave);

  // When hovering a wave node, filter the markdown to just that wave's
  // `### [[wave-N-...]]` section. Falls back to full markdown otherwise.
  const memoryMarkdown = useMemo(() => {
    const full = memoryQ.data ?? "";
    if (!full || !hoveredId || !hoveredId.startsWith(WAVE_ID_PREFIX)) return full;
    const waveNum = Number.parseInt(hoveredId.slice(WAVE_ID_PREFIX.length), 10);
    if (!Number.isFinite(waveNum)) return full;
    return filterMarkdownToWave(full, waveNum);
  }, [memoryQ.data, hoveredId]);

  function isVisible(id: string): boolean {
    if (!hoveredId) return true;
    if (id === hoveredId) return true;
    return neighbors.get(hoveredId)?.has(id) ?? false;
  }

  function isEdgeVisible(e: GraphEdge): boolean {
    if (!hoveredId) return true;
    return e.from === hoveredId || e.to === hoveredId;
  }

  const isLoading =
    childrenQ.isLoading || wavesQ.isLoading || (memoryQ.isLoading && currentWave != null);

  if (isLoading && placed.length <= 1) {
    return (
      <div className="pt-2">
        <div className="h-72 rounded bg-muted/30 animate-pulse" />
      </div>
    );
  }

  if (placed.length === 0) {
    return (
      <EmptyState
        title="Sem rede para renderizar"
        description="Esta spec não tem ondas nem sub-specs ligadas."
      />
    );
  }

  return (
    <div className="flex flex-col md:flex-row md:gap-4 min-h-[600px] flex-1 pt-2">
      <div
        ref={canvasRef}
        className="flex-1 min-w-0 min-h-[500px] rounded border border-border bg-card/20 text-foreground"
      >
        <svg
          width="100%"
          height="100%"
          viewBox={`0 0 ${size.w} ${size.h}`}
          role="img"
          aria-label={`Grafo de rede da spec ${specName}`}
          className="block w-full h-full"
          preserveAspectRatio="xMidYMid meet"
        >
          {/* Edges first so nodes paint on top */}
          <g fill="none">
            {edges.map((e, i) => {
              const a = placedById.get(e.from);
              const b = placedById.get(e.to);
              if (!a || !b) return null;
              const visible = isEdgeVisible(e);
              const isChain = e.kind === "wave-chain";
              return (
                <line
                  key={`edge-${i}`}
                  x1={a.x}
                  y1={a.y}
                  x2={b.x}
                  y2={b.y}
                  stroke="currentColor"
                  opacity={visible ? (isChain ? 0.55 : 0.7) : 0.12}
                  strokeWidth={isChain ? 1 : 1.5}
                  strokeDasharray={isChain ? "4 2" : undefined}
                />
              );
            })}
          </g>

          {/* Nodes */}
          {placed.map((n) => {
            const visible = isVisible(n.id);
            const hovered = n.id === hoveredId;
            const r = radiusFor(n.kind, hovered);
            return (
              <g
                key={n.id}
                style={{ cursor: "pointer" }}
                opacity={visible ? 1 : 0.3}
                onMouseEnter={() => setHoveredId(n.id)}
                onMouseLeave={() => setHoveredId((curr) => (curr === n.id ? null : curr))}
              >
                <title>{n.label}</title>
                <circle
                  cx={n.x}
                  cy={n.y}
                  r={r}
                  fill={colorFor(n.kind)}
                  stroke="var(--color-background)"
                  strokeWidth={1.5}
                />
                <text
                  x={n.x}
                  y={n.y + r + 10}
                  textAnchor="middle"
                  fill="currentColor"
                  pointerEvents="none"
                  className={labelSizeFor(n.kind)}
                >
                  <tspan>{truncate(n.label, 28)}</tspan>
                </text>
              </g>
            );
          })}
        </svg>
      </div>

      <aside className="w-full md:w-80 max-h-[600px] overflow-auto rounded border border-border bg-card/20 p-3 mt-3 md:mt-0">
        <h3 className="text-xs uppercase tracking-wide text-muted-foreground mb-2">
          Memórias de ondas anteriores
        </h3>
        {currentWave == null ? (
          <p className="text-[12px] text-muted-foreground/70 italic">
            Sem ondas registradas — nenhuma memória para mostrar.
          </p>
        ) : memoryQ.isLoading ? (
          <div className="h-24 rounded bg-muted/30 animate-pulse" />
        ) : memoryQ.error ? (
          <p className="text-[12px] text-destructive">
            Erro ao carregar memória: {(memoryQ.error as Error).message}
          </p>
        ) : !(memoryMarkdown ?? "").trim() ? (
          <p className="text-[12px] text-muted-foreground/70 italic">
            {hoveredId && hoveredId.startsWith(WAVE_ID_PREFIX)
              ? "Sem memória registrada para esta onda."
              : "Sem memória registrada para ondas anteriores."}
          </p>
        ) : (
          <Markdown content={memoryMarkdown} />
        )}
      </aside>
    </div>
  );
}

function truncate(s: string, max: number): string {
  return s.length <= max ? s : `${s.slice(0, max - 1)}…`;
}

/** Pull just the `### [[wave-N-...]]` section out of the cross-wave markdown.
 *  Returns the original markdown when no matching section is found so the
 *  sidebar never goes empty just because hover misses. */
function filterMarkdownToWave(markdown: string, waveNum: number): string {
  const lines = markdown.split(/\r?\n/);
  const header = new RegExp(`^###\\s+\\[\\[wave-${waveNum}-`);
  let start = -1;
  for (let i = 0; i < lines.length; i++) {
    if (header.test(lines[i])) {
      start = i;
      break;
    }
  }
  if (start === -1) return markdown;
  let end = lines.length;
  for (let i = start + 1; i < lines.length; i++) {
    if (/^###\s+\[\[wave-/.test(lines[i])) {
      end = i;
      break;
    }
  }
  return lines.slice(start, end).join("\n");
}
