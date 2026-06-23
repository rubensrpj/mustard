import { useMemo, useState } from "react";
import {
  ChevronRight,
  CheckCircle2,
  XCircle,
  Loader2,
  Circle,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useNavigate } from "react-router";
import { cn } from "@/lib/utils";
import { relativeTime } from "@/lib/time";
import type { SpecChild, SpecWave } from "@/lib/types/specs";
import type { WaveChecklistProgress } from "@/lib/dashboard";
import { useSpecChecklistProgress } from "@/hooks/useSpecChecklistProgress";
import { useSpecWaveFiles } from "@/hooks/useSpecWaveFiles";
import { useSpecWavesPlanned } from "@/hooks/useSpecWavesPlanned";
import { mergeWaves } from "../_shared/merge-waves";
import { WaveMarkdownDrawer } from "../WaveMarkdownDrawer";
import { StatusPill } from "../_shared/spec-status";
import { SplitDetail } from "@/components/layout";
import { useT } from "@/lib/i18n";

interface SpecWavesTabProps {
  waves: SpecWave[];
  /** Wave 2 (2026-05-21) — when set, each wave row becomes clickable. The
   *  parent (`SpecDetailDashboard`) opens the markdown drawer in response. */
  onOpenWave?: (wave: number) => void;
  /** Project repo path; forwarded to per-row `useSpecWaveFiles` so each row
   *  can show the real `## Arquivos` count from the wave sub-spec. */
  repoPath?: string | null;
  /** Spec name; forwarded to per-row `useSpecWaveFiles`. */
  spec?: string;
  /**
   * Wave 2 polish — sub-specs linked to the parent. Passed in by
   * `SpecDrillDown` (which owns `useSpecChildren`) so this tab can render
   * each child nested under the wave whose `started_at` window contains the
   * child's `started_at`. Children with `wave == null` go to the "Sem onda
   * correlacionada" bucket.
   */
  subSpecs?: SpecChild[];
  /** Currently-selected wave — drives the always-open split panel and the
   *  highlight on the matching row. `null` = no wave selected (no panel). */
  openWave?: number | null;
}

/**
 * Wave status palette — mirrors `mustard_specsdb::WaveStatus`.
 * Mustard yellow for the active wave, --intent-success for completed,
 * --intent-error for failed, neutral grey for queued. AC-12 of
 * spec 2026-05-20-dashboard-ux-honest pins this file as the one
 * that must read wave.status + render formatDuration.
 */
const STATUS_CLS: Record<string, string> = {
  completed:   "bg-[--intent-success]/15 text-[--intent-success]",
  in_progress: "bg-[--primary]/15 text-[--primary]",
  failed:      "bg-[--intent-error]/15 text-[--intent-error]",
  queued:      "bg-muted text-muted-foreground",
};

const STATUS_LABEL_KEYS: Record<string, string> = {
  completed:   "specWaves.status.completed",
  in_progress: "specWaves.status.in_progress",
  failed:      "specWaves.status.failed",
  queued:      "specWaves.status.queued",
};

/**
 * Per-status marker icon for the wave row. The icon makes the wave's stage
 * legible at a glance independent of the colour-coded pill: a completed wave
 * shows a static check (never the in-progress spinner), the live wave spins,
 * a failed wave shows an X, and a queued wave shows an empty ring. `spin`
 * gates the only animated marker so a finished wave reads as settled.
 */
const STATUS_ICON: Record<string, { Icon: LucideIcon; cls: string; spin?: boolean }> = {
  completed:   { Icon: CheckCircle2, cls: "text-[--intent-success]" },
  in_progress: { Icon: Loader2,     cls: "text-[--primary]", spin: true },
  failed:      { Icon: XCircle,     cls: "text-[--intent-error]" },
  queued:      { Icon: Circle,      cls: "text-muted-foreground/50" },
};

/**
 * Wave 4 (spec `2026-05-21-dashboard-spec-tabs-polish`): source badge palette
 * for the sub-spec rows. `event` = surfaced via SQLite `spec.link` (live
 * telemetry → sky), `header` = scanned from the on-disk `### Parent:` block
 * (declarative → amber), `both` = present in both sources (the healthy
 * steady-state → emerald).
 */
const SOURCE_CLS: Record<string, string> = {
  event:  "bg-sky-500/15 text-sky-400",
  header: "bg-amber-500/15 text-amber-400",
  both:   "bg-emerald-500/15 text-emerald-400",
};

const SOURCE_LABEL_KEYS: Record<string, string> = {
  event:  "specWaves.source.event",
  header: "specWaves.source.header",
  both:   "specWaves.source.both",
};

/** Format milliseconds into a compact "1h 2m" / "12s" string. */
function formatDuration(ms: number | null): string {
  if (ms == null || ms <= 0) return "—";
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  const sec = s % 60;
  if (m < 60) return sec > 0 ? `${m}m ${sec}s` : `${m}m`;
  const h = Math.floor(m / 60);
  const min = m % 60;
  return min > 0 ? `${h}h ${min}m` : `${h}h`;
}

/** Compute duration_ms from started_at/completed_at when present. */
function waveDurationMs(wave: SpecWave): number | null {
  if (!wave.started_at) return null;
  const start = new Date(wave.started_at).getTime();
  const end = wave.completed_at
    ? new Date(wave.completed_at).getTime()
    : wave.status === "in_progress"
      ? Date.now()
      : null;
  if (end == null) return null;
  const diff = end - start;
  return Number.isFinite(diff) && diff >= 0 ? diff : null;
}

/** Compute duration_ms for a sub-spec child (used inside the expanded row). */
function childDurationMs(child: SpecChild): number | null {
  if (!child.started_at) return null;
  const start = new Date(child.started_at).getTime();
  const end = child.completed_at
    ? new Date(child.completed_at).getTime()
    : null;
  if (end == null) return null;
  const diff = end - start;
  return Number.isFinite(diff) && diff >= 0 ? diff : null;
}

/**
 * One child row rendered inside an expanded wave. Mirrors the visual
 * vocabulary of the legacy `SpecChildrenTab` (status pill + slug +
 * duration), kept compact so a wave with several children stays scannable.
 */
function ChildRow({ child }: { child: SpecChild }) {
  const t = useT();
  const navigate = useNavigate();
  const duration = childDurationMs(child);
  return (
    <li>
      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation();
          navigate(`/specs#${child.spec}`);
        }}
        className={cn(
          "w-full flex items-center gap-2 py-1.5 px-2 text-left rounded",
          "hover:bg-muted/40 focus-visible:outline-none focus-visible:ring-2",
          "focus-visible:ring-[--primary] transition-colors",
        )}
        title={t("specWaves.child.openTitle").replace("{spec}", child.spec)}
      >
        <span
          className="font-mono text-[11px] truncate flex-1 min-w-0"
          title={child.spec}
        >
          {child.spec}
        </span>
        {child.source && SOURCE_CLS[child.source] && (
          <span
            className={cn(
              "text-[10px] font-medium px-1.5 py-0.5 rounded tracking-wide uppercase shrink-0",
              SOURCE_CLS[child.source],
            )}
            title={
              child.source === "event"
                ? t("specWaves.child.source.event")
                : child.source === "header"
                  ? t("specWaves.child.source.header")
                  : t("specWaves.child.source.both")
            }
          >
            {t(SOURCE_LABEL_KEYS[child.source])}
          </span>
        )}
        <StatusPill status={child.status} />
        {duration != null && (
          <span
            className="text-[10px] text-muted-foreground tabular-nums"
            style={{ fontVariantNumeric: "tabular-nums" }}
            title={t("specWaves.child.durationTitle")}
          >
            {formatDuration(duration)}
          </span>
        )}
        {child.reason && (
          <span
            className="text-[10px] text-muted-foreground/70 italic truncate max-w-[140px]"
            title={child.reason}
          >
            {child.reason}
          </span>
        )}
      </button>
    </li>
  );
}

interface WaveLiProps {
  wave: SpecWave;
  onOpenWave?: (wave: number) => void;
  repoPath?: string | null;
  spec?: string;
  childrenOfWave?: SpecChild[];
  /** Visual override for the row border — used by the Onda #0 (parent) row. */
  borderOverride?: string;
  /** Optional label shown after the wave number (used by Onda #0). */
  labelOverride?: string;
  /**
   * Wave 3 (spec `checklist-progresso-por-onda`): per-wave checklist progress
   * (`done`/`total` trackable items). `null`/`undefined` when the wave has no
   * checklist data — the row honestly renders nothing instead of `0/0`.
   */
  checklist?: WaveChecklistProgress | null;
  /** Whether this row is the currently-selected wave (panel open for it).
   *  Drives a persistent highlight so the user sees which wave the split
   *  panel is showing. */
  selected?: boolean;
}

/**
 * One row in the waves list. Lives as its own component so each row can run
 * its own `useSpecWaveFiles` query (React Query dedupes by key when the
 * `WaveMarkdownDrawer` opens for the same wave) and surface the real file
 * count from the wave sub-spec's `## Arquivos` block. Falls back to
 * `wave.files_changed` (events-derived runtime count) while the query is
 * loading or when the wave sub-spec doesn't exist.
 *
 * Wave 2 (spec polish): now expandable — a chevron at the left of the row
 * toggles a nested sub-spec list (children correlated to this wave via
 * `child.wave`). The chevron click does NOT propagate to the row-click that
 * opens the markdown drawer.
 */
function WaveLi({
  wave,
  onOpenWave,
  repoPath,
  spec,
  childrenOfWave,
  borderOverride,
  labelOverride,
  checklist,
  selected,
}: WaveLiProps) {
  const t = useT();
  const filesQ = useSpecWaveFiles(repoPath ?? null, spec ?? "", wave.wave);
  const duration_ms = waveDurationMs(wave);
  const isFailed = wave.status === "failed";
  const childCount = childrenOfWave?.length ?? 0;
  const [expanded, setExpanded] = useState<boolean>(false);

  // Show the real `## Arquivos` count when the query has data and the wave
  // sub-spec actually exists (path != null). Otherwise fall back to the
  // events-derived `files_changed` so the row never looks empty during the
  // brief gap between mount and first response.
  const realCount =
    filesQ.data && filesQ.data.path != null ? filesQ.data.count : null;
  const displayCount = realCount ?? wave.files_changed;
  const countTitle =
    realCount != null
      ? t("specWaves.row.fileCountTitle.declared")
      : t("specWaves.row.fileCountTitle.touched");

  const borderClass =
    borderOverride ??
    (wave.status === "completed"
      ? "border-[--intent-success]/30"
      : wave.status === "in_progress"
        ? "border-[--primary]/40"
        : isFailed
          ? "border-[--intent-error]/40"
          : "border-border/50");

  const clickable = !!onOpenWave;
  const handleOpen = () => onOpenWave?.(wave.wave);
  const handleKeyDown = (e: React.KeyboardEvent<HTMLLIElement>) => {
    if (!clickable) return;
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      handleOpen();
    }
  };

  return (
    <li
      className={cn(
        "flex flex-col gap-1.5 px-3 py-2.5 rounded-md border bg-card/10",
        borderClass,
        clickable &&
          "cursor-pointer hover:bg-card/30 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[--primary]",
        selected && "bg-[--primary]/10 ring-1 ring-[--primary]/60",
      )}
      aria-current={selected ? "true" : undefined}
      onClick={clickable ? handleOpen : undefined}
      onKeyDown={clickable ? handleKeyDown : undefined}
      role={clickable ? "button" : undefined}
      tabIndex={clickable ? 0 : undefined}
      aria-label={clickable ? t("specWaves.row.openWaveAria").replace("{n}", String(wave.wave)) : undefined}
    >
      <div className="flex items-start gap-3">
        {/* Chevron expand toggle — only when this wave has correlated children */}
        {childCount > 0 ? (
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              setExpanded((v) => !v);
            }}
            aria-label={expanded ? t("specWaves.row.collapseAria") : t("specWaves.row.expandAria")}
            aria-expanded={expanded}
            title={
              expanded
                ? t("specWaves.row.collapseTitle")
                : t("specWaves.row.expandTitle").replace("{count}", String(childCount)).replace("{plural}", childCount === 1 ? "" : "s")
            }
            className="shrink-0 mt-0.5 h-5 w-5 flex items-center justify-center rounded text-muted-foreground hover:text-foreground hover:bg-muted/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[--primary] transition-colors"
          >
            <ChevronRight
              className={cn(
                "h-3.5 w-3.5 transition-transform",
                expanded && "rotate-90",
              )}
              aria-hidden
            />
          </button>
        ) : (
          // Keep a fixed-width placeholder so wave numbers line up vertically
          <span className="shrink-0 mt-0.5 h-5 w-5" aria-hidden />
        )}

        {/* Status marker — distinct icon per wave.status so a completed wave
            never shows the in-progress spinner. */}
        {(() => {
          const marker = STATUS_ICON[wave.status];
          if (!marker) return null;
          const { Icon, cls, spin } = marker;
          return (
            <Icon
              className={cn(
                "h-3.5 w-3.5 shrink-0 mt-0.5",
                cls,
                spin && "animate-spin",
              )}
              aria-hidden
            />
          );
        })()}

        {/* Wave number */}
        <span
          className="text-[12px] font-mono font-medium text-muted-foreground shrink-0 tabular-nums pt-0.5"
          style={{ fontVariantNumeric: "tabular-nums" }}
        >
          #{wave.wave}
        </span>

        {/* Role + agent */}
        <div className="flex-1 min-w-0 flex flex-col gap-0.5">
          <div className="flex items-center gap-2 flex-wrap">
            {labelOverride ? (
              <span className="text-[12px] font-medium text-foreground/80 truncate">
                {labelOverride}
              </span>
            ) : wave.role ? (
              <span className="text-[12px] font-medium text-foreground/80 truncate">
                {wave.role}
              </span>
            ) : null}
            {wave.agent_type && (
              <span className="text-[11px] font-mono text-muted-foreground/70 bg-muted px-1 rounded">
                {wave.agent_type}
              </span>
            )}
            {childCount > 0 && (
              <span
                className="text-[10px] font-mono font-medium px-1.5 py-0.5 rounded uppercase tracking-wide bg-muted/60 text-muted-foreground"
                title={t("specWaves.row.subSpecsTitle").replace("{count}", String(childCount))}
              >
                +{childCount} sub-spec{childCount === 1 ? "" : "s"}
              </span>
            )}
          </div>

          {wave.summary && (
            <p
              className="text-[11px] text-muted-foreground/80 truncate"
              title={wave.summary}
            >
              {wave.summary}
            </p>
          )}

          <div
            className="flex items-center gap-3 text-[11px] text-muted-foreground flex-wrap tabular-nums"
            style={{ fontVariantNumeric: "tabular-nums" }}
          >
            {wave.started_at && (
              <span>{t("specWaves.row.startedAt")} {relativeTime(wave.started_at)}</span>
            )}
            {wave.completed_at && (
              <span>{t("specWaves.row.completedAt")} {relativeTime(wave.completed_at)}</span>
            )}
            {duration_ms != null && (
              <span title={t("specWaves.row.durationTitle")}>
                {t("specWaves.row.durationLabel")} {formatDuration(duration_ms)}
              </span>
            )}
            <span title={countTitle}>
              {displayCount}{" "}
              {displayCount === 1 ? t("specWaves.row.fileSingular") : t("specWaves.row.filePlural")}
            </span>
            {/* Wave 3 (checklist-progresso-por-onda): N/M checklist items.
                Only rendered when the wave actually has checklist data —
                no fabricated 0/0 for checklist-less waves. */}
            {checklist && (checklist.total > 0 || checklist.done > 0) && (
              <span title={t("specWaves.row.checklistTitle")}>
                {checklist.total > 0
                  ? t("specWaves.row.checklistCount")
                      .replace("{done}", String(checklist.done))
                      .replace("{total}", String(checklist.total))
                  : t("specWaves.row.checklistDoneOnly").replace(
                      "{done}",
                      String(checklist.done),
                    )}
              </span>
            )}
          </div>
        </div>

        {/* Running marker — the prominent "EXECUTANDO" badge on the live wave,
            so the user can tell at a glance WHICH wave is executing. Uses the
            mustard/warning accent so it pops next to the neutral status pill. */}
        {wave.status === "in_progress" && (
          <span
            className="text-[10px] font-semibold px-1.5 py-0.5 rounded uppercase tracking-wide shrink-0 bg-[--primary] text-[--primary-foreground] animate-pulse"
            title={t("specWaves.row.runningBadgeTitle")}
          >
            {t("specWaves.row.runningBadge")}
          </span>
        )}

        {/* Status pill — driven by wave.status */}
        <span
          className={cn(
            "text-[10px] font-medium px-1.5 py-0.5 rounded uppercase tracking-wide shrink-0",
            STATUS_CLS[wave.status] ?? "bg-muted text-muted-foreground",
          )}
        >
          {STATUS_LABEL_KEYS[wave.status] ? t(STATUS_LABEL_KEYS[wave.status]) : wave.status}
        </span>
      </div>

      {/* Last-error preview when the wave failed. The shape currently
          doesn't carry a structured error blob, so the "ver detalhes"
          hint nudges the user to open the markdown viewer or QA tab
          where the actual stderr lives. */}
      {isFailed && (
        <p className="text-[11px] text-[--intent-error]/80 pl-7">
          {t("specWaves.row.waveFailed")}
        </p>
      )}

      {/* Nested sub-specs — only when expanded and there's at least one
          correlated child. We render inside the same <li> so the visual
          grouping is obvious and the keyboard order stays sensible. */}
      {expanded && childCount > 0 && (
        <ul
          className="flex flex-col gap-0.5 pl-9 pt-1 border-t border-border/40 mt-1"
          onClick={(e) => e.stopPropagation()}
        >
          {childrenOfWave!.map((child) => (
            <ChildRow key={child.spec} child={child} />
          ))}
        </ul>
      )}
    </li>
  );
}

export function SpecWavesTab({
  waves,
  onOpenWave,
  repoPath,
  spec,
  subSpecs,
  openWave,
}: SpecWavesTabProps) {
  const t = useT();
  // Bug 5 fix (spec `2026-05-21-dashboard-spec-tabs-polish` W1): during
  // EXECUTE the SQLite projection (`waves`) might be empty or partial
  // because the wave events have not landed yet. Union it with the wave
  // plan scanned from `<repo>/.claude/spec/{spec}/wave-N-{role}/` so the
  // user always sees the declared structure. Events-derived rows take
  // precedence (they have timestamps); plan-only rows render as queued.
  const plannedQ = useSpecWavesPlanned(repoPath ?? null, spec ?? null);
  const merged = useMemo<SpecWave[]>(
    () => mergeWaves(waves, plannedQ.data),
    [waves, plannedQ.data],
  );

  // Wave 3 (checklist-progresso-por-onda): per-wave checklist progress,
  // indexed by wave number for the row renderer. Missing data (no sidecars,
  // no events) resolves to an empty array → no badge on any row.
  const checklistQ = useSpecChecklistProgress(repoPath ?? null, spec ?? null);
  const checklistByWave = useMemo(() => {
    const byWave = new Map<number, WaveChecklistProgress>();
    for (const row of checklistQ.data ?? []) byWave.set(row.wave, row);
    return byWave;
  }, [checklistQ.data]);

  // Wave 2 (spec polish): bucket children by their correlated wave. Each
  // entry maps wave number → SpecChild[]; the leftover (`wave == null` /
  // undefined) goes into `orphans` and renders in the "Sem onda" group at
  // the bottom.
  const { childrenByWave, orphans } = useMemo(() => {
    const byWave = new Map<number, SpecChild[]>();
    const orphanList: SpecChild[] = [];
    for (const c of subSpecs ?? []) {
      if (c.wave == null) {
        orphanList.push(c);
      } else {
        const arr = byWave.get(c.wave) ?? [];
        arr.push(c);
        byWave.set(c.wave, arr);
      }
    }
    return { childrenByWave: byWave, orphans: orphanList };
  }, [subSpecs]);

  // Wave 2 polish: synthetic "Onda #0" row pointing at the parent
  // (wave-plan.md or spec.md). Always rendered at the top of the list when
  // we know the spec name and have a repo path. `wave: 0` lights up
  // `resolve_wave_spec_path` in `mustard-rt run wave-files`.
  const ondaZero = useMemo<SpecWave | null>(() => {
    if (!spec) return null;
    return {
      wave: 0,
      role: t("specWaves.row.specPrincipal"),
      status: "completed",
      started_at: null,
      completed_at: null,
      agent_type: null,
      files_changed: 0,
    };
  }, [spec, t]);

  const openWaveRole =
    openWave != null
      ? openWave === 0
        ? t("specWaves.row.specPrincipal")
        : (merged.find((w) => w.wave === openWave)?.role ?? null)
      : null;

  // Whether the selected NUMBERED wave is completed — drives `forceChecked` in
  // the markdown panel so a finished wave's `## Tarefas` renders as marked even
  // when its on-disk spec.md still reads `[ ]`. Onda #0 (the synthetic parent /
  // full spec narrative) is intentionally excluded: its hard-coded "completed"
  // status doesn't reflect the spec's real lifecycle, so we leave the full
  // narrative's checklists honest rather than risk marking an active spec.
  const openWaveCompleted =
    openWave != null && openWave !== 0
      ? merged.find((w) => w.wave === openWave)?.status === "completed"
      : false;

  const listEmpty = merged.length === 0;

  if (listEmpty && !ondaZero) {
    // Only truly empty: no events AND no wave-plan on disk AND no spec
    // name to render the Onda #0 anchor.
    if (plannedQ.isLoading) {
      return (
        <ul className="flex flex-col gap-2 pt-1">
          {[0, 1, 2].map((i) => (
            <li
              key={i}
              className="h-14 rounded-md bg-muted/40 animate-pulse"
            />
          ))}
        </ul>
      );
    }
    return (
      <p className="text-[13px] text-muted-foreground py-4 text-center">
        {t("specWaves.empty")}
      </p>
    );
  }

  // Layout: when a wave is selected the markdown panel sits beside the list
  // inside an always-open resizable split (spec
  // `melhorias-no-dashboard-destacar-projeto`, wave 2). No wave selected →
  // the list renders full-width.
  const showPanel = openWave != null;

  const list = (
    <ul className="flex flex-col gap-2">
      {ondaZero && (
        <WaveLi
          key="onda-zero"
          wave={ondaZero}
          onOpenWave={onOpenWave}
          repoPath={repoPath}
          spec={spec}
          // No children correlation for Onda #0 (children are nested under
          // numbered waves; orphans render in the bucket at the bottom).
          childrenOfWave={undefined}
          borderOverride="border-[--primary]/40 bg-[--primary]/5"
          labelOverride={t("specWaves.row.mainSpecLabel")}
          checklist={checklistByWave.get(0) ?? null}
          selected={openWave === 0}
        />
      )}
      {merged.map((wave) => (
        <WaveLi
          key={wave.wave}
          wave={wave}
          onOpenWave={onOpenWave}
          repoPath={repoPath}
          spec={spec}
          childrenOfWave={childrenByWave.get(wave.wave)}
          checklist={checklistByWave.get(wave.wave) ?? null}
          selected={openWave === wave.wave}
        />
      ))}
      {orphans.length > 0 && (
        <li
          className="flex flex-col gap-2 px-3 py-2.5 rounded-md border border-dashed border-border/50 bg-card/5"
          aria-label={t("specWaves.orphans.aria")}
        >
          <div className="flex items-center gap-2 text-[11px] text-muted-foreground uppercase tracking-wide">
            <span>{t("specWaves.orphans.label")}</span>
            <span
              className="tabular-nums"
              style={{ fontVariantNumeric: "tabular-nums" }}
            >
              ({orphans.length})
            </span>
          </div>
          <ul className="flex flex-col gap-0.5 pl-2">
            {orphans.map((child) => (
              <ChildRow key={child.spec} child={child} />
            ))}
          </ul>
        </li>
      )}
    </ul>
  );

  // No wave selected → the list owns the full width.
  if (!showPanel) {
    return list;
  }

  // A wave is selected → always-open resizable split: the waves list on the
  // left, the wave markdown panel on the right, divided by a draggable handle.
  // `min-h-[40vh]` gives the split a sensible height inside the Ondas tab; the
  // panel border matches the legacy inline `<aside>` chrome.
  return (
    <SplitDetail
      open
      variant="inline"
      initialFraction={0.5}
      panel={
        <div className="h-full rounded-md border border-border bg-card/30 overflow-hidden">
          <WaveMarkdownDrawer
            repoPath={repoPath ?? null}
            spec={spec ?? ""}
            wave={openWave}
            role={openWaveRole}
            waveCompleted={openWaveCompleted}
          />
        </div>
      }
    >
      <div className="min-h-[40vh]">{list}</div>
    </SplitDetail>
  );
}
