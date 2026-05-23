// Wave-5 spec `2026-05-21-dashboard-spec-tabs`: lightweight Fruchterman-Reingold
// force-directed layout for the spec network graph. Pure function — no DOM, no
// React hooks, no un-seeded `Math.random()`. Returns NEW node objects with
// updated `x`/`y` so callers can memoize on the result reference.

export type NodeKind = "parent" | "wave" | "subspec";
export type EdgeKind = "wave-chain" | "parent-child" | "spec-link";

export interface GraphNode {
  id: string;
  label: string;
  kind: NodeKind;
  x: number;
  y: number;
}

export interface GraphEdge {
  from: string;
  to: string;
  kind: EdgeKind;
}

export interface SimulateOptions {
  iterations?: number;
  width?: number;
  height?: number;
  /** Optional seed for deterministic initial placement when nodes have no
   *  starting positions. Same seed + same input = same output. */
  seed?: number;
}

/** mulberry32 — tiny deterministic PRNG. Returns a function that yields a
 *  pseudo-random number in `[0, 1)` per call. */
function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/**
 * Runs a Fruchterman-Reingold simulation over the provided graph. Returns a
 * NEW array of nodes with updated positions; the input is not mutated.
 *
 * Repulsion `k^2 / d` between all pairs; attraction `d^2 / k` along edges;
 * temperature cools linearly from `width / 10` to 0 across the iterations.
 * Positions are clamped to `[0, width] × [0, height]`.
 */
export function simulate(
  nodes: GraphNode[],
  edges: GraphEdge[],
  opts?: SimulateOptions,
): GraphNode[] {
  const iterations = opts?.iterations ?? 60;
  const width = opts?.width ?? 800;
  const height = opts?.height ?? 600;
  const n = nodes.length;

  if (n === 0) return [];

  // Deterministic initial placement: nodes go around a circle centered on
  // the canvas. The seeded RNG only nudges them off-axis so identical
  // co-linear nodes don't get stuck on top of each other.
  const rand = mulberry32(opts?.seed ?? 1);
  const cx = width / 2;
  const cy = height / 2;
  const radius = Math.min(width, height) * 0.35;
  const placed = nodes.map((node, i) => {
    const angle = (2 * Math.PI * i) / Math.max(n, 1);
    const jitter = (rand() - 0.5) * 4;
    return {
      ...node,
      x: cx + Math.cos(angle) * radius + jitter,
      y: cy + Math.sin(angle) * radius + jitter,
    };
  });

  const k = Math.sqrt((width * height) / Math.max(n, 1));
  const initialTemp = width / 10;

  // Build id→index lookup once for the edge attraction pass.
  const idx = new Map<string, number>();
  for (let i = 0; i < placed.length; i++) idx.set(placed[i].id, i);

  for (let iter = 0; iter < iterations; iter++) {
    const t = initialTemp * (1 - iter / iterations);
    const dx = new Array<number>(n).fill(0);
    const dy = new Array<number>(n).fill(0);

    // Repulsion between every pair.
    for (let i = 0; i < n; i++) {
      for (let j = i + 1; j < n; j++) {
        let vx = placed[i].x - placed[j].x;
        let vy = placed[i].y - placed[j].y;
        let dist = Math.sqrt(vx * vx + vy * vy);
        if (dist < 0.01) {
          // Same point — nudge deterministically.
          vx = (rand() - 0.5) * 0.1;
          vy = (rand() - 0.5) * 0.1;
          dist = Math.sqrt(vx * vx + vy * vy) || 0.01;
        }
        const force = (k * k) / dist;
        const ux = vx / dist;
        const uy = vy / dist;
        dx[i] += ux * force;
        dy[i] += uy * force;
        dx[j] -= ux * force;
        dy[j] -= uy * force;
      }
    }

    // Attraction along edges.
    for (const edge of edges) {
      const a = idx.get(edge.from);
      const b = idx.get(edge.to);
      if (a == null || b == null) continue;
      let vx = placed[a].x - placed[b].x;
      let vy = placed[a].y - placed[b].y;
      const dist = Math.sqrt(vx * vx + vy * vy) || 0.01;
      const force = (dist * dist) / k;
      const ux = vx / dist;
      const uy = vy / dist;
      dx[a] -= ux * force;
      dy[a] -= uy * force;
      dx[b] += ux * force;
      dy[b] += uy * force;
    }

    // Apply displacement bounded by temperature, then clamp to canvas.
    for (let i = 0; i < n; i++) {
      const disp = Math.sqrt(dx[i] * dx[i] + dy[i] * dy[i]) || 0.01;
      const step = Math.min(disp, t);
      placed[i] = {
        ...placed[i],
        x: clamp(placed[i].x + (dx[i] / disp) * step, 0, width),
        y: clamp(placed[i].y + (dy[i] / disp) * step, 0, height),
      };
    }
  }

  return placed;
}

function clamp(v: number, lo: number, hi: number): number {
  return v < lo ? lo : v > hi ? hi : v;
}

export interface RadialLayoutOptions {
  width?: number;
  height?: number;
  /** Ratio of `min(w,h)` used as the wave orbit radius (anel 1). */
  r1Ratio?: number;
  /** Ratio of `min(w,h)` used as the sub-spec orbit radius (anel 2). */
  r2Ratio?: number;
  /** Total angular spread (radians) used to fan sub-specs around their wave. */
  subspecSpread?: number;
}

/**
 * Deterministic radial "mind-map" layout. Places the parent at the canvas
 * center, distributes wave nodes evenly on an outer ring, and clusters each
 * wave's sub-specs in an arc around that wave. No RNG, no iteration, no
 * mutation — caller can memoize on the returned reference.
 *
 * Sub-spec → wave attachment is resolved via edges of kind `spec-link`:
 * if any `wave-N → sub` edge exists, the sub is grouped under that wave.
 * Otherwise (e.g. parent → sub direct links from the current builder) we
 * fall back to distributing those subs evenly across all waves so the
 * graph stays balanced. Nodes with no resolvable category land on the
 * outer ring as a passthrough.
 */
export function radialLayout(
  nodes: GraphNode[],
  edges: GraphEdge[],
  opts?: RadialLayoutOptions,
): GraphNode[] {
  const width = opts?.width ?? 800;
  const height = opts?.height ?? 600;
  const r1Ratio = opts?.r1Ratio ?? 0.3;
  const r2Ratio = opts?.r2Ratio ?? 0.15;
  const subspecSpread = opts?.subspecSpread ?? Math.PI / 3;

  if (nodes.length === 0) return [];

  const cx = width / 2;
  const cy = height / 2;
  const R1 = Math.min(width, height) * r1Ratio;
  const R2 = Math.min(width, height) * r2Ratio;

  const parent = nodes.find((n) => n.kind === "parent");
  const waves = nodes
    .filter((n) => n.kind === "wave")
    .slice()
    .sort((a, b) => a.id.localeCompare(b.id, undefined, { numeric: true }));
  const subspecs = nodes.filter((n) => n.kind === "subspec");

  // Resolve wave parent for each sub-spec via `spec-link` edges originating
  // from a wave node. Build a quick lookup of wave ids.
  const waveIdSet = new Set(waves.map((w) => w.id));
  const subToWave = new Map<string, string>();
  for (const e of edges) {
    if (e.kind !== "spec-link") continue;
    if (waveIdSet.has(e.from)) {
      subToWave.set(e.to, e.from);
    } else if (waveIdSet.has(e.to)) {
      subToWave.set(e.from, e.to);
    }
  }

  // Bucket sub-specs by wave id. Unattached subs go to a synthetic bucket
  // so they can be evenly redistributed across waves below.
  const wavePos = new Map<string, { x: number; y: number; angle: number }>();
  const N = waves.length;
  waves.forEach((w, i) => {
    const angle = N > 0 ? (2 * Math.PI * i) / N - Math.PI / 2 : -Math.PI / 2;
    wavePos.set(w.id, {
      x: cx + R1 * Math.cos(angle),
      y: cy + R1 * Math.sin(angle),
      angle,
    });
  });

  const byWave = new Map<string, GraphNode[]>();
  for (const w of waves) byWave.set(w.id, []);
  const unattached: GraphNode[] = [];
  for (const s of subspecs) {
    const wid = subToWave.get(s.id);
    if (wid && byWave.has(wid)) {
      byWave.get(wid)!.push(s);
    } else {
      unattached.push(s);
    }
  }
  // Round-robin distribute unattached subs across waves so the radial layout
  // stays balanced when the builder only links parent→sub.
  if (N > 0) {
    unattached.forEach((s, i) => {
      const wid = waves[i % N].id;
      byWave.get(wid)!.push(s);
    });
  }

  // Stable sort sub-specs within each wave so position is deterministic.
  for (const arr of byWave.values()) {
    arr.sort((a, b) => a.id.localeCompare(b.id, undefined, { numeric: true }));
  }

  const placedById = new Map<string, GraphNode>();

  if (parent) {
    placedById.set(parent.id, { ...parent, x: cx, y: cy });
  }

  for (const w of waves) {
    const pos = wavePos.get(w.id)!;
    placedById.set(w.id, { ...w, x: pos.x, y: pos.y });
  }

  for (const w of waves) {
    const subs = byWave.get(w.id) ?? [];
    if (subs.length === 0) continue;
    const { x: wx, y: wy, angle: theta } = wavePos.get(w.id)!;
    const M = subs.length;
    const half = subspecSpread / 2;
    subs.forEach((s, j) => {
      const t = M > 1 ? j / (M - 1) : 0.5;
      const alpha = theta - half + subspecSpread * t;
      placedById.set(s.id, {
        ...s,
        x: wx + R2 * Math.cos(alpha),
        y: wy + R2 * Math.sin(alpha),
      });
    });
  }

  // Any node not categorized (unknown kind, or subspec when there are no
  // waves) lands on the outer ring as a graceful passthrough.
  const fallback = nodes.filter((n) => !placedById.has(n.id));
  if (fallback.length > 0) {
    const baseR = Math.min(width, height) * (N === 0 ? r1Ratio : r1Ratio + r2Ratio);
    fallback.forEach((n, i) => {
      const angle = (2 * Math.PI * i) / Math.max(fallback.length, 1) - Math.PI / 2;
      placedById.set(n.id, {
        ...n,
        x: cx + baseR * Math.cos(angle),
        y: cy + baseR * Math.sin(angle),
      });
    });
  }

  // Preserve input order in the output.
  return nodes.map((n) => placedById.get(n.id) ?? { ...n, x: cx, y: cy });
}
