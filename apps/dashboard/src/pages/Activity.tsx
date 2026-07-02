// Activity — the project's work, grouped by human-readable work TYPE.
//
// Replaces the Specs/Sessions nav entries. Each session is one unit of work; we
// group by its `pipeline.kind` work-type (`SessionRow.kind`: feature / bugfix /
// task / tactical-fix) and relabel that to a human heading ("Nova
// funcionalidade", "Correção", "Ajuste", "Mudança rápida"). Grouping by `kind`
// — NOT `category` — reveals the lean `task`/`bugfix` fast-paths: those emit a
// `pipeline.kind` event but never a `skill.invoked`, so their `category` (the
// skill suffix) is null and grouping by it would dump them in the loose bucket,
// invisible-by-type. `category` is the FALLBACK for older runs predating
// `pipeline.kind` (see `groupingKey`).
//
// DEPTH-ADAPTIVE detail: what a row reveals depends on whether it is spec-backed.
//   - Spec-backed items (grouped as `feature` AND carrying a `last_spec`): the
//     spec's pipeline journey — at-a-glance chips on the CLOSED row (waves, QA,
//     stage, from the batched spec cards) and, on expand, a stage rail + the
//     waves with per-wave progress + the QA/AC breakdown + a deep-link INTO the
//     existing spec drill-in (`/specs#{slug}`). The rich spec UI is reused,
//     never duplicated.
//   - Lean items (task/bugfix/tactical-fix, no spec): the run narrative —
//     request → changes → outcome — plus a trace link and a muted note that the
//     work is single-layer (no waves/PRD/QA); NO stage rail (absence = leanness).
//
// Spec progress for the closed-row chips comes from ONE batched query
// (`fetchSpecCards` → `dashboard_spec_cards`, a single workspace fold), keyed by
// slug and looked up per spec-backed row — never one query per row. The expand
// additionally lazy-loads the per-wave / per-AC lists.

import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "react-router";
import { ChevronRight, ArrowUpRight } from "lucide-react";
import { useStore } from "@/lib/store";
import {
  fetchSessions,
  fetchSpecCards,
  useActiveProjectName,
  type SessionRow,
} from "@/lib/dashboard";
import {
  PageSurface,
  EditorialBand,
  DataCard,
  EmptyState,
  StatusDot,
  CollapsibleGroup,
  AcBreakdown,
} from "@/components/page";
import { Badge } from "@/components/ui/badge";
import { useT } from "@/lib/i18n";
import { relativeTime } from "@/lib/time";
import { cn } from "@/lib/utils";
import { useSpecWaves } from "@/hooks/useSpecWaves";
import { useSpecQuality } from "@/hooks/useSpecQuality";
import { stateFromStatus } from "@/features/specs/_shared/stage-from-status";
import type { Stage, SpecCard } from "@/lib/types/specs";

// Sentinel bucket key for a `null` category (a session with no command).
const NULL_BUCKET = "__null__";

// Fixed front of the section order, by work TYPE.
const PRIORITY_ORDER = ["feature", "task", "bugfix", "tactical-fix", "analyze"];

// The grouping key for one session: the `pipeline.kind` work-type when present,
// else the `category` (skill suffix) fallback, else the loose null bucket.
function groupingKey(s: SessionRow): string {
  return s.kind ?? s.category ?? NULL_BUCKET;
}

// Resolve a grouping key to its human work-TYPE heading.
function kindLabel(t: ReturnType<typeof useT>, key: string): string {
  const dictKey = key === NULL_BUCKET ? "activity.kind.__null__" : `activity.kind.${key}`;
  const mapped = t(dictKey, "");
  if (mapped) return mapped;
  return key.charAt(0).toUpperCase() + key.slice(1);
}

// Compact "Mudanças" line: distinct files + tool count.
function changesText(session: SessionRow): string {
  const files = `${session.files_touched} ${session.files_touched === 1 ? "arquivo" : "arquivos"}`;
  const tools = `${session.tools_used} ${session.tools_used === 1 ? "tool" : "tools"}`;
  return `${files} · ${tools}`;
}

// A row is spec-backed (gets the pipeline journey) when it is grouped as a
// `feature` AND carries a `last_spec` to drill into. Gating on the GROUPING key
// (`kind ?? category`), NOT `kind` alone, is essential: older feature runs
// predate `pipeline.kind` (kind is null) and are grouped via `category`, so a
// `kind === "feature"` test wrongly rendered them as lean — the "igual ao
// antigo" bug. `last_spec` is the handle the journey + deep-links need.
function isSpecBacked(session: SessionRow): session is SessionRow & { last_spec: string } {
  return groupingKey(session) === "feature" && !!session.last_spec;
}

// Map a lifecycle stage to its short human label (reuses the rail's keys).
function stageLabel(t: ReturnType<typeof useT>, stage: Stage): string {
  const key = stage === "qa-review" ? "qa" : stage;
  return t(`activity.stage.${key}`, key);
}

// ── Stage rail ────────────────────────────────────────────────────────────
// Horizontal ANALYZE → PLAN → EXECUTE → QA → CLOSE indicator. The lone bold
// element of the redesign — the SAME phase tokens (`--color-phase-*`) and the
// `stage-bullet-pulse` keyframe declared in style.css that StageBullet uses.
const RAIL_STAGES: { stage: Stage; var: string; key: string }[] = [
  { stage: "analyze", var: "analyze", key: "activity.stage.analyze" },
  { stage: "plan", var: "plan", key: "activity.stage.plan" },
  { stage: "execute", var: "execute", key: "activity.stage.execute" },
  { stage: "qa-review", var: "qa", key: "activity.stage.qa" },
  { stage: "close", var: "close", key: "activity.stage.close" },
];

function StageRail({ status }: { status: string }) {
  const t = useT();
  const { stage, outcome } = stateFromStatus(status);
  const terminal = outcome !== "active";
  const currentIdx = RAIL_STAGES.findIndex((s) => s.stage === stage);
  return (
    <div className="flex items-center overflow-x-auto" role="img" aria-label={t("activity.journey")}>
      {RAIL_STAGES.map((s, i) => {
        const done = terminal || i < currentIdx;
        const current = !terminal && i === currentIdx;
        const dotColor = done
          ? "var(--color-phase-execute)"
          : current
            ? `var(--color-phase-${s.var})`
            : "var(--color-muted-foreground, #5b636e)";
        return (
          <div key={s.stage} className="flex items-center shrink-0">
            <span className="flex items-center gap-2">
              <span
                className={cn("w-2.5 h-2.5 rounded-full border-2", current && "stage-bullet-pulse")}
                style={{ borderColor: dotColor, backgroundColor: done || current ? dotColor : "transparent" }}
                aria-hidden
              />
              <span
                className={cn(
                  "text-[10.5px] uppercase tracking-wide tabular-nums",
                  done ? "text-foreground" : current ? "font-semibold" : "text-muted-foreground/60",
                )}
                style={current ? { color: `var(--color-phase-${s.var})` } : undefined}
              >
                {t(s.key)}
              </span>
            </span>
            {i < RAIL_STAGES.length - 1 && (
              <span
                className="w-8 h-0.5 mx-1.5 shrink-0"
                style={{ backgroundColor: i < currentIdx || terminal ? "var(--color-phase-execute)" : "var(--border, #2b3139)" }}
                aria-hidden
              />
            )}
          </div>
        );
      })}
    </div>
  );
}

// At-a-glance chips on a CLOSED spec-backed row: waves done/total, QA pass/total,
// and the stage/outcome — all from the batched spec card (no per-row query). This
// is the "depth visible without expanding" the redesign promises.
function SpecChips({ card }: { card: SpecCard }) {
  const t = useT();
  const wavesDone = card.current_wave ?? 0;
  const total = card.total_waves;
  return (
    <span className="inline-flex items-center gap-1.5 flex-wrap">
      {total != null && total > 0 && (
        <span
          className="inline-flex items-center gap-1 text-[10px] px-1.5 py-0.5 rounded-full border tabular-nums"
          style={{ borderColor: "var(--border)" }}
        >
          <span className="w-1.5 h-1.5 rounded-full" style={{ background: "var(--color-phase-execute)" }} aria-hidden />
          {t("activity.chip.waves", "Ondas")} {wavesDone}/{total}
        </span>
      )}
      {card.ac_total > 0 && (
        <span
          className="inline-flex items-center gap-1 text-[10px] px-1.5 py-0.5 rounded-full border tabular-nums"
          style={{ color: "var(--color-phase-qa)", borderColor: "rgb(139 92 246 / 0.25)", background: "var(--color-phase-qa-bg, rgb(139 92 246 / 0.1))" }}
        >
          QA {card.ac_passed}/{card.ac_total}
        </span>
      )}
    </span>
  );
}

// Status for a spec-backed row: the SPEC lifecycle (stage/outcome), so the dot
// AND label agree with the chips and the colour always means the same thing —
// gray "Encerrada" when the spec is done, the phase colour of its current stage
// when still active (e.g. green "Executando"). A closed work session whose spec
// is still mid-pipeline therefore reads by its real stage, not a misleading
// "Encerrado". Fixes the "dot always gray / some closed rows coloured" mismatch.
function SpecStatus({ card }: { card: SpecCard }) {
  const t = useT();
  const { stage, outcome } = stateFromStatus(card.status);
  const terminal = outcome !== "active";
  const color = terminal
    ? "var(--color-phase-close)"
    : `var(--color-phase-${stage === "qa-review" ? "qa" : stage})`;
  return (
    <span className="inline-flex items-center gap-1.5 shrink-0">
      <span className="w-2 h-2 rounded-full shrink-0" style={{ background: color }} aria-hidden />
      <span className="text-[10px] uppercase tracking-wide" style={{ color }}>
        {terminal ? t("activity.chip.closed", "Encerrada") : stageLabel(t, stage)}
      </span>
    </span>
  );
}

// ── Spec journey (expand) ───────────────────────────────────────────────────
// Rendered inside an open spec-backed row. The stage rail + headline counts come
// from the already-batched `card`; the per-wave and per-AC LISTS lazy-load here
// (mounted only on expand), keyed on `[..., repoPath, spec]`, watcher/poll-driven,
// each fail-open to empty.
function SpecJourney({
  repoPath,
  spec,
  card,
  fallbackStatus,
  onOpenSpec,
}: {
  repoPath: string | null;
  spec: string;
  card: SpecCard | undefined;
  /** The session's own status — the rail's source until the card resolves. */
  fallbackStatus: string;
  onOpenSpec: () => void;
}) {
  const t = useT();
  const wavesQ = useSpecWaves(repoPath, spec);
  const qualityQ = useSpecQuality(repoPath, spec);

  const railStatus = card?.status ?? fallbackStatus;
  const waves = wavesQ.data ?? [];
  const totalWaves = card?.total_waves ?? (waves.length || null);
  const doneWaves = waves.filter((w) => w.status === "completed").length;

  const quality = qualityQ.data ?? [];
  const acPass = quality.filter((q) => q.status === "pass").length;
  const acFail = quality.filter((q) => q.status === "fail").length;
  const acSkip = quality.filter((q) => q.status === "skip").length;

  return (
    <div className="flex flex-col gap-4 text-[11px]">
      <div className="flex flex-col gap-1.5">
        <span className="uppercase tracking-wide text-[10px] text-muted-foreground/70">
          {t("activity.journey")}
        </span>
        <StageRail status={railStatus} />
      </div>

      <div className="flex flex-col gap-1.5">
        <span className="uppercase tracking-wide text-[10px] text-muted-foreground/70">
          {t("activity.waves")}
          {totalWaves != null && (
            <span className="ml-1.5 text-foreground/70 tabular-nums">
              {doneWaves}/{totalWaves}
            </span>
          )}
        </span>
        {wavesQ.isLoading ? (
          <div className="h-4 rounded bg-muted/40 animate-pulse" />
        ) : waves.length === 0 ? (
          <span className="text-muted-foreground/60">{t("activity.waves.empty")}</span>
        ) : (
          <ul className="flex flex-col gap-1">
            {waves.map((w) => {
              const variant =
                w.status === "completed" ? "success" : w.status === "in_progress" ? "planning" : w.status === "failed" ? "error" : "idle";
              return (
                <li key={w.wave} className="flex items-center gap-2 tabular-nums">
                  <StatusDot variant={variant} pulse={w.status === "in_progress"} size="sm" />
                  <span className="font-mono text-[11px] text-foreground/90">#{w.wave}</span>
                  {w.role && (
                    <span className="text-[10px] uppercase tracking-wide text-muted-foreground">{w.role}</span>
                  )}
                  <span className="text-muted-foreground/80">{t(`activity.waveStatus.${w.status}`, w.status)}</span>
                </li>
              );
            })}
          </ul>
        )}
      </div>

      <div className="flex flex-col gap-1.5">
        <span className="uppercase tracking-wide text-[10px] text-muted-foreground/70">
          {t("activity.quality")}
        </span>
        {qualityQ.isLoading ? (
          <div className="h-4 w-32 rounded bg-muted/40 animate-pulse" />
        ) : quality.length === 0 ? (
          <span className="text-muted-foreground/60">{t("activity.quality.empty")}</span>
        ) : (
          <AcBreakdown pass={acPass} fail={acFail} skip={acSkip} />
        )}
      </div>

      <div className="flex items-center gap-3 flex-wrap pt-0.5">
        <button
          type="button"
          onClick={onOpenSpec}
          className="inline-flex items-center gap-1 text-primary font-medium hover:underline"
        >
          {t("activity.openSpec")}
          <ArrowUpRight className="h-3 w-3" aria-hidden />
        </button>
        <span className="font-mono text-[10px] text-muted-foreground/70 truncate" title={spec}>
          {spec}
        </span>
      </div>
    </div>
  );
}

function ActivityRow({
  session,
  cardsBySpec,
}: {
  session: SessionRow;
  cardsBySpec: Map<string, SpecCard>;
}) {
  const t = useT();
  const navigate = useNavigate();
  const projectsRoot = useStore((s) => s.projectsRoot);
  const specBacked = isSpecBacked(session);
  const card = specBacked ? cardsBySpec.get(session.last_spec) : undefined;
  const handle = session.is_unknown_bucket
    ? t("activity.unattributed")
    : session.slug || session.id;
  const title = session.title ?? t("activity.untitled");
  const isOpen = session.status === "open";
  const startedRel = relativeTime(session.started_at);
  const activeRel = session.last_activity_at ? relativeTime(session.last_activity_at) : null;
  const filesTitle = session.files.length > 0 ? session.files.join("\n") : "Nenhum arquivo registrado.";

  // Deep-link into the existing spec drill-in. The Specs page reads
  // `window.location.hash` on mount and `openSpec(hash)` when it matches a
  // date-prefixed slug — the SAME mechanism `SpecWavesTab` child rows use.
  const openSpec = () => {
    if (session.last_spec) navigate(`/specs#${session.last_spec}`);
  };

  return (
    <li
      className={cn(
        "border-b border-border/40 last:border-b-0",
        session.is_unknown_bucket && "opacity-70",
      )}
    >
      <details className="group">
        <summary className={cn("flex items-center gap-3 px-3 py-2.5 cursor-pointer list-none", "hover:bg-muted/30 transition-colors")}>
          {/* Spec-backed items carry a mustard kind-tick; lean items a neutral one. */}
          <span
            aria-hidden
            className={cn("self-stretch w-0.5 rounded shrink-0", specBacked ? "bg-primary" : "bg-border")}
          />
          {specBacked && card ? (
            <SpecStatus card={card} />
          ) : (
            <span className="inline-flex items-center gap-1.5 shrink-0">
              <StatusDot variant={isOpen ? "active" : "done"} pulse={isOpen} size="sm" />
              <span className="text-[10px] uppercase tracking-wide text-muted-foreground">
                {isOpen ? t("activity.outcome.open") : t("activity.outcome.closed")}
              </span>
            </span>
          )}
          <div className="flex flex-col flex-1 min-w-0">
            <span className="font-medium text-[12px] truncate text-foreground" title={title}>
              {title}
            </span>
            <div className="flex items-center gap-2 text-[11px] text-foreground/80 flex-wrap">
              {/* Spec-backed rows lead with the journey chips (waves/QA/stage);
                  every row then shows its changes line. */}
              {specBacked && card && <SpecChips card={card} />}
              <span className="font-medium" title={filesTitle}>
                {changesText(session)}
              </span>
              {session.scope && (
                <Badge variant="outline" className="text-[10px] py-0 uppercase tracking-wide">
                  {session.scope}
                </Badge>
              )}
              <span aria-hidden className="text-muted-foreground">·</span>
              <span className="text-muted-foreground">started {startedRel}</span>
              {activeRel && (
                <>
                  <span aria-hidden className="text-muted-foreground">·</span>
                  <span className="text-muted-foreground">last {activeRel}</span>
                </>
              )}
            </div>
          </div>
          <ChevronRight
            aria-hidden
            className="h-3.5 w-3.5 shrink-0 text-muted-foreground/50 transition-transform group-open:rotate-90"
          />
        </summary>

        {/* Depth-adaptive detail: spec-backed → the pipeline journey; lean → narrative. */}
        <div className="px-3 pb-3 pt-1 pl-[2.1rem]">
          {specBacked ? (
            <SpecJourney
              repoPath={projectsRoot ?? null}
              spec={session.last_spec}
              card={card}
              fallbackStatus={session.status}
              onOpenSpec={openSpec}
            />
          ) : (
            <div className="flex flex-col gap-2 text-[11px]">
              <div className="flex flex-col gap-0.5">
                <span className="uppercase tracking-wide text-[10px] text-muted-foreground/70">
                  {t("activity.narrative.request")}
                </span>
                <span className="text-foreground/90">{title}</span>
              </div>

              <div className="flex flex-col gap-0.5">
                <span className="uppercase tracking-wide text-[10px] text-muted-foreground/70">
                  {t("activity.narrative.changes")}
                </span>
                <span className="text-foreground/90">{changesText(session)}</span>
                {session.tool_breakdown.length > 0 && (
                  <span className="text-muted-foreground">
                    {session.tool_breakdown.slice(0, 6).map((tc) => `${tc.count} ${tc.name}`).join(" · ")}
                  </span>
                )}
                {session.files.length > 0 && (
                  <ul className="mt-0.5 flex flex-col gap-0.5 text-muted-foreground">
                    {session.files.slice(0, 8).map((f) => (
                      <li key={f} className="font-mono truncate" title={f}>
                        {f}
                      </li>
                    ))}
                    {session.files.length > 8 && (
                      <li className="text-muted-foreground/60">+{session.files.length - 8}</li>
                    )}
                  </ul>
                )}
              </div>

              <div className="flex flex-col gap-0.5">
                <span className="uppercase tracking-wide text-[10px] text-muted-foreground/70">
                  {t("activity.narrative.outcome")}
                </span>
                <span className="text-foreground/90">
                  {isOpen ? t("activity.outcome.open") : t("activity.outcome.closed")}
                </span>
              </div>

              <div className="flex items-center gap-2 pt-0.5 flex-wrap">
                <Link
                  to={`/sessions/${encodeURIComponent(session.id)}`}
                  className="inline-flex items-center gap-1 text-primary hover:underline"
                >
                  {t("activity.narrative.openTrace")}
                  <ArrowUpRight className="h-3 w-3" aria-hidden />
                </Link>
                <span className="text-muted-foreground/60 italic">{t("activity.leanNote")}</span>
                <span aria-hidden className="text-muted-foreground/50">·</span>
                <span className="font-mono text-[10px] text-muted-foreground/70 truncate" title={handle}>
                  {handle}
                </span>
              </div>
            </div>
          )}
        </div>
      </details>
    </li>
  );
}

interface ActivitySection {
  key: string;
  label: string;
  sessions: SessionRow[];
}

function groupByKind(
  data: SessionRow[] | undefined,
  t: ReturnType<typeof useT>,
): ActivitySection[] | undefined {
  if (!data) return undefined;
  const buckets = new Map<string, SessionRow[]>();
  for (const s of data) {
    const key = groupingKey(s);
    const bucket = buckets.get(key);
    if (bucket) bucket.push(s);
    else buckets.set(key, [s]);
  }
  for (const bucket of buckets.values()) {
    bucket.sort((a, b) => Number(a.is_unknown_bucket) - Number(b.is_unknown_bucket));
  }
  const keys = [...buckets.keys()];
  keys.sort((a, b) => {
    if (a === NULL_BUCKET) return 1;
    if (b === NULL_BUCKET) return -1;
    const ai = PRIORITY_ORDER.indexOf(a);
    const bi = PRIORITY_ORDER.indexOf(b);
    if (ai !== -1 && bi !== -1) return ai - bi;
    if (ai !== -1) return -1;
    if (bi !== -1) return 1;
    return kindLabel(t, a).localeCompare(kindLabel(t, b));
  });
  return keys.map((key) => ({
    key,
    label: kindLabel(t, key),
    sessions: buckets.get(key)!,
  }));
}

// The three explainer cards above the list — they encode HOW to read the view
// (grouping, the stage rail, spec access), matching the approved design.
function ActivityNote() {
  const t = useT();
  const cards: { title: string; body: string }[] = [
    { title: t("activity.note.grouped.title", "Agrupado por tipo"), body: t("activity.note.grouped.body", "feature · ajuste · correção · mudança rápida — revela até o trabalho enxuto que nunca virou spec.") },
    { title: t("activity.note.rail.title", "Trilho de estágios"), body: t("activity.note.rail.body", "Só funcionalidades mostram o caminho Analyze→Close. A ausência dele já diz que o item é enxuto.") },
    { title: t("activity.note.spec.title", "Spec a um toque"), body: t("activity.note.spec.body", "Ondas, PRD, Spec e QA abrem o drill-in que já existe — nada foi perdido.") },
  ];
  return (
    <div className="grid gap-3 mb-4 sm:grid-cols-3">
      {cards.map((c) => (
        <div key={c.title} className="rounded-md border border-border border-l-2 border-l-primary bg-card px-3 py-2.5">
          <div className="text-[12px] font-medium mb-0.5">{c.title}</div>
          <div className="text-[11px] text-muted-foreground leading-snug">{c.body}</div>
        </div>
      ))}
    </div>
  );
}

export function Activity() {
  const t = useT();
  const projectsRoot = useStore((s) => s.projectsRoot);
  const activeProjectName = useActiveProjectName();

  const { data, isLoading, error } = useQuery<SessionRow[]>({
    queryKey: ["sessions", projectsRoot],
    queryFn: () => fetchSessions(projectsRoot!),
    enabled: !!projectsRoot,
    staleTime: 30_000,
  });

  // One batched fold of every top-level spec card → the closed-row journey chips
  // (waves/QA/stage) for spec-backed rows. Fail-open to empty; watcher refreshes
  // `["spec-cards", repoPath]` on event writes.
  const { data: specCards } = useQuery<SpecCard[]>({
    queryKey: ["spec-cards", projectsRoot],
    queryFn: () => fetchSpecCards(projectsRoot!),
    enabled: !!projectsRoot,
    staleTime: 30_000,
  });

  const cardsBySpec = useMemo(() => {
    const m = new Map<string, SpecCard>();
    for (const c of specCards ?? []) m.set(c.spec, c);
    return m;
  }, [specCards]);

  const sections = useMemo(() => groupByKind(data, t), [data, t]);

  if (!projectsRoot) {
    return (
      <PageSurface>
        <EmptyState
          title={t("activity.empty.noProject.title")}
          description={t("activity.empty.noProject.description")}
        />
      </PageSurface>
    );
  }

  return (
    <PageSurface>
      <EditorialBand
        eyebrow="Atividade"
        title={t("activity.editorialTitle")}
        subtitle={
          activeProjectName
            ? t("activity.editorialSubtitle.named").replace("{name}", activeProjectName)
            : t("activity.editorialSubtitle")
        }
      />
      {sections && sections.length > 0 && <ActivityNote />}
      {isLoading && (
        <div className="flex flex-col gap-2">
          {[0, 1, 2].map((i) => (
            <div key={i} className="h-12 rounded bg-muted/40 animate-pulse" />
          ))}
        </div>
      )}
      {error && <p className="text-destructive text-sm">{(error as Error).message}</p>}
      {sections && sections.length === 0 && (
        <EmptyState
          title={t("activity.empty.none.title")}
          description={t("activity.empty.none.description")}
        />
      )}
      {sections && sections.length > 0 && (
        <div className="flex flex-col gap-3">
          {sections.map((section, idx) => (
            <CollapsibleGroup
              key={section.key}
              label={section.label}
              count={section.sessions.length}
              defaultOpen={idx === 0}
            >
              <DataCard>
                <ul className="flex flex-col">
                  {section.sessions.map((s) => (
                    <ActivityRow key={s.id} session={s} cardsBySpec={cardsBySpec} />
                  ))}
                </ul>
              </DataCard>
            </CollapsibleGroup>
          ))}
        </div>
      )}
    </PageSurface>
  );
}
