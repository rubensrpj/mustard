import { useMemo } from "react";
import { useQuery, useQueries } from "@tanstack/react-query";
import { useNavigate } from "react-router";
import {
  dashboardWikilinkExtract,
  dashboardMemoryCrossWave,
  type WikilinkExtract,
} from "@/lib/dashboard";
import { EmptyState } from "@/components/page";
import { Markdown } from "@/components/Markdown";

interface SpecNetworkTabProps {
  /** Absolute path to the active project repo. The backend resolves the spec
   *  directory under `<repoPath>/.claude/spec/{bucket}/<specName>/`. */
  repoPath: string | null;
  /** Spec name as stored under `.claude/spec/{bucket}/<specName>/`. The
   *  backend resolves the filesystem path; the frontend only ever passes the
   *  name, never a raw directory. */
  specName: string;
}

/** Detect the wave number from a spec name like `wave-3-dashboard-graph` →
 *  3. Returns `null` for non-wave names. */
function waveNumberOf(name: string): number | null {
  const match = /^wave-(\d+)/i.exec(name);
  if (!match) return null;
  const n = Number.parseInt(match[1], 10);
  return Number.isFinite(n) ? n : null;
}

/** Classify a wikilink target relative to `specName`:
 *  - "parent": looks like a date-prefixed spec (`YYYY-MM-DD-...`) and isn't
 *    the current spec itself
 *  - "wave": matches `wave-N-...`
 *  - "dependent": anything else (sibling cross-spec link)
 *  - "self": the current spec — dropped from the graph */
type Layer = "parent" | "wave" | "dependent" | "self";
function classify(target: string, specName: string): Layer {
  if (target === specName) return "self";
  if (/^wave-\d+/i.test(target)) return "wave";
  if (/^\d{4}-\d{2}-\d{2}/.test(target)) return "parent";
  return "dependent";
}

interface GraphNode {
  name: string;
  layer: Exclude<Layer, "self">;
  /** Wave number when `layer === "wave"`, else null. Drives the middle-row
   *  ordering. */
  wave: number | null;
}

function buildGraph(extract: WikilinkExtract, specName: string): GraphNode[] {
  const seen = new Map<string, GraphNode>();
  for (const link of extract.wikilinks) {
    const layer = classify(link.to, specName);
    if (layer === "self") continue;
    if (seen.has(link.to)) continue;
    seen.set(link.to, {
      name: link.to,
      layer,
      wave: layer === "wave" ? waveNumberOf(link.to) : null,
    });
  }
  return [...seen.values()];
}

// SVG layout constants. Static positions on purpose — the wave spec's
// non-goal block explicitly rules out a force-directed layout.
const NODE_W = 180;
const NODE_H = 36;
const NODE_RX = 6;
const COL_GAP = 24;
const ROW_GAP = 80;
const PADDING = 24;

interface PlacedNode extends GraphNode {
  x: number;
  y: number;
}

function layout(nodes: GraphNode[], centerX: number) {
  const parents = nodes.filter((n) => n.layer === "parent");
  const waves = nodes
    .filter((n) => n.layer === "wave")
    .sort((a, b) => (a.wave ?? 0) - (b.wave ?? 0));
  const dependents = nodes.filter((n) => n.layer === "dependent");

  function placeRow(row: GraphNode[], y: number): PlacedNode[] {
    if (row.length === 0) return [];
    const totalWidth = row.length * NODE_W + (row.length - 1) * COL_GAP;
    const startX = centerX - totalWidth / 2;
    return row.map((n, i) => ({
      ...n,
      x: startX + i * (NODE_W + COL_GAP),
      y,
    }));
  }

  const yParent = PADDING;
  const yWave = yParent + NODE_H + ROW_GAP;
  const yDep = yWave + NODE_H + ROW_GAP;

  return {
    parents: placeRow(parents, yParent),
    waves: placeRow(waves, yWave),
    dependents: placeRow(dependents, yDep),
    height: yDep + NODE_H + PADDING,
  };
}

/**
 * SpecNetworkTab — Wave-3 wikilink graph + per-wave cross-wave memory.
 *
 * The graph is intentionally simple: three static vertical layers (parent at
 * the top, waves grouped in the middle ordered by number, dependents at the
 * bottom). No force-directed layout — the wave spec explicitly lists dynamic
 * layout as a non-goal. Clicking a node calls `navigate('/specs#<name>')`,
 * which the Specs page already honours by auto-expanding the matching row.
 */
export function SpecNetworkTab({ repoPath, specName }: SpecNetworkTabProps) {
  const navigate = useNavigate();

  const extractQ = useQuery({
    queryKey: ["wikilink-extract", repoPath, specName],
    queryFn: () => dashboardWikilinkExtract(repoPath!, specName),
    enabled: !!repoPath && !!specName,
    staleTime: 30_000,
  });

  const nodes = useMemo<GraphNode[]>(() => {
    if (!extractQ.data) return [];
    return buildGraph(extractQ.data, specName);
  }, [extractQ.data, specName]);

  const waveNumbers = useMemo(() => {
    return nodes
      .filter((n) => n.layer === "wave" && n.wave !== null)
      .map((n) => n.wave as number)
      .sort((a, b) => a - b);
  }, [nodes]);

  // Fan out one query per detected wave. Memory comes from the parent spec
  // (wave-plan name), so we key by `(specName, wave+1)` per the spec — the
  // command's `--wave` flag is 1-based and represents the *current* wave;
  // we ask "what did previous waves leave for wave N+1?".
  const memoryQueries = useQueries({
    queries: waveNumbers.map((w) => ({
      queryKey: ["memory-cross-wave", repoPath, specName, w + 1],
      queryFn: () => dashboardMemoryCrossWave(repoPath!, specName, w + 1),
      enabled: !!repoPath,
      staleTime: 60_000,
    })),
  });

  if (extractQ.isLoading) {
    return (
      <div className="pt-2">
        <div className="h-48 rounded bg-muted/30 animate-pulse" />
      </div>
    );
  }
  if (extractQ.error) {
    return (
      <EmptyState
        variant="error"
        title="Erro ao carregar rede"
        description={extractQ.error.message}
      />
    );
  }
  if (nodes.length === 0) {
    return (
      <EmptyState
        title="Sem wikilinks detectados"
        description="Esta spec não tem wave-files ou referências [[name]]."
      />
    );
  }

  // Compute SVG dimensions once nodes are placed.
  const viewWidth = 720;
  const placed = layout(nodes, viewWidth / 2);

  function handleNodeClick(name: string) {
    navigate(`/specs#${name}`);
  }

  // Edges: parent → current (anchor), current → each wave, current → dependents.
  // We render an implicit "current spec" anchor in the middle layer between
  // parent and waves rows for visual clarity.
  const anchorX = viewWidth / 2 - NODE_W / 2;
  const anchorY = PADDING + (NODE_H + ROW_GAP) / 2 - NODE_H / 2;

  return (
    <div className="flex flex-col gap-6 pt-2">
      <section>
        <h3 className="text-sm font-medium text-foreground mb-2">Grafo</h3>
        <div className="rounded-lg border border-border bg-card/30 overflow-hidden">
          <svg
            width="100%"
            viewBox={`0 0 ${viewWidth} ${placed.height + ROW_GAP}`}
            role="img"
            aria-label={`Grafo de wikilinks da spec ${specName}`}
            className="block"
          >
            {/* Edges */}
            <g stroke="rgb(99 102 241 / 0.4)" strokeWidth={1}>
              {placed.parents.map((p) => (
                <line
                  key={`e-p-${p.name}`}
                  x1={p.x + NODE_W / 2}
                  y1={p.y + NODE_H}
                  x2={anchorX + NODE_W / 2}
                  y2={anchorY}
                />
              ))}
              {placed.waves.map((w) => (
                <line
                  key={`e-w-${w.name}`}
                  x1={anchorX + NODE_W / 2}
                  y1={anchorY + NODE_H}
                  x2={w.x + NODE_W / 2}
                  y2={w.y}
                />
              ))}
              {placed.dependents.map((d) => (
                <line
                  key={`e-d-${d.name}`}
                  x1={anchorX + NODE_W / 2}
                  y1={anchorY + NODE_H}
                  x2={d.x + NODE_W / 2}
                  y2={d.y}
                />
              ))}
            </g>

            {/* Anchor (current spec) */}
            <g>
              <rect
                x={anchorX}
                y={anchorY}
                width={NODE_W}
                height={NODE_H}
                rx={NODE_RX}
                fill="rgb(99 102 241 / 0.18)"
                stroke="rgb(99 102 241 / 0.6)"
              />
              <text
                x={anchorX + NODE_W / 2}
                y={anchorY + NODE_H / 2 + 4}
                textAnchor="middle"
                fontSize={12}
                fill="currentColor"
                className="font-medium"
              >
                {truncate(specName, 22)}
              </text>
            </g>

            {/* Parent / waves / dependents nodes */}
            {[...placed.parents, ...placed.waves, ...placed.dependents].map(
              (n) => (
                <g
                  key={`n-${n.name}`}
                  style={{ cursor: "pointer" }}
                  onClick={() => handleNodeClick(n.name)}
                >
                  <title>{n.name}</title>
                  <rect
                    x={n.x}
                    y={n.y}
                    width={NODE_W}
                    height={NODE_H}
                    rx={NODE_RX}
                    fill={nodeFill(n.layer)}
                    stroke={nodeStroke(n.layer)}
                  />
                  <text
                    x={n.x + NODE_W / 2}
                    y={n.y + NODE_H / 2 + 4}
                    textAnchor="middle"
                    fontSize={12}
                    fill="currentColor"
                  >
                    {truncate(n.name, 22)}
                  </text>
                </g>
              ),
            )}

            {/* Row labels */}
            <g fontSize={10} fill="currentColor" opacity={0.5}>
              <text x={8} y={PADDING + NODE_H / 2 + 4}>
                parent
              </text>
              <text x={8} y={placed.waves[0]?.y ?? anchorY + NODE_H + 30}>
                ondas
              </text>
              <text x={8} y={(placed.dependents[0]?.y ?? 0) + NODE_H / 2 + 4}>
                dependentes
              </text>
            </g>
          </svg>
        </div>
      </section>

      <section>
        <h3 className="text-sm font-medium text-foreground mb-2">
          Memória por onda
        </h3>
        {waveNumbers.length === 0 ? (
          <EmptyState
            title="Sem ondas detectadas"
            description="Esta spec não decompõe em ondas — não há memória cross-wave para exibir."
          />
        ) : (
          <div className="flex flex-col gap-3">
            {waveNumbers.map((w, idx) => {
              const q = memoryQueries[idx];
              const content = (q.data ?? "").trim();
              return (
                <div
                  key={`mem-${w}`}
                  className="rounded-lg border border-border bg-card/30 p-3"
                >
                  <div className="text-xs font-medium text-muted-foreground mb-1">
                    Onda {w} → {w + 1}
                  </div>
                  {q.isLoading ? (
                    <div className="h-6 rounded bg-muted/30 animate-pulse" />
                  ) : content.length === 0 ? (
                    <p className="text-[12px] text-muted-foreground/70 italic">
                      Sem memória registrada para a próxima onda.
                    </p>
                  ) : (
                    <Markdown content={content} />
                  )}
                </div>
              );
            })}
          </div>
        )}
      </section>
    </div>
  );
}

function truncate(s: string, max: number) {
  return s.length <= max ? s : `${s.slice(0, max - 1)}…`;
}

function nodeFill(layer: GraphNode["layer"]) {
  switch (layer) {
    case "parent":
      return "rgb(168 85 247 / 0.15)"; // violet
    case "wave":
      return "rgb(99 102 241 / 0.12)"; // indigo
    case "dependent":
      return "rgb(148 163 184 / 0.12)"; // slate
  }
}

function nodeStroke(layer: GraphNode["layer"]) {
  switch (layer) {
    case "parent":
      return "rgb(168 85 247 / 0.55)";
    case "wave":
      return "rgb(99 102 241 / 0.5)";
    case "dependent":
      return "rgb(148 163 184 / 0.45)";
  }
}
