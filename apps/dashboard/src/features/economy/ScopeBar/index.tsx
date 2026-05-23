// ScopeBar — 4-button scope picker for the W7 Economia page.
//
// Emits an `EconomyScope` (the discriminated union in `lib/types/economy.ts`)
// whenever the user touches one of the four toggles. Spec/Wave variants need
// extra context (which spec, which wave); Comparar projetos needs a list of
// project paths. We resolve all three locally — specs via the standard
// `dashboard_specs` query, projects via `useProjects()` — so the parent page
// only deals with the resolved scope.
//
// AC-5 contract: the page MUST include the four labels "Projeto", "Spec",
// "Wave", "Comparar". They live on the buttons below — do not rename.

import { useEffect, useMemo, useState, type ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Folder, FileText, Waves, GitCompare } from "lucide-react";
import { cn } from "@/lib/utils";
import { fetchSpecs, useProjects } from "@/lib/dashboard";
import type { SpecRow } from "@/lib/dashboard";
import type { EconomyScope, EconomyScopeKind } from "@/lib/types/economy";
import {
  projectScope,
  specScope,
  waveScope,
  allProjectsScope,
} from "@/lib/types/economy";

export interface ScopeBarProps {
  /** Currently selected project root (the workspace). */
  projectPath: string;
  /** Current scope — drives which sub-dropdown is open. */
  scope: EconomyScope;
  onScopeChange: (scope: EconomyScope) => void;
}

// Static metadata (kind + icon) for the four scope tabs. Labels are resolved
// at render time via `t()` so the bar swaps language without a remount.
const TAB_META: Array<{ kind: EconomyScopeKind; labelKey: string; icon: ReactNode }> = [
  { kind: "project",      labelKey: "economy.scope.project", icon: <Folder size={14} /> },
  { kind: "spec",         labelKey: "economy.scope.spec",    icon: <FileText size={14} /> },
  { kind: "wave",         labelKey: "economy.scope.wave",    icon: <Waves size={14} /> },
  { kind: "all_projects", labelKey: "economy.scope.compare", icon: <GitCompare size={14} /> },
];

export function ScopeBar({ projectPath, scope, onScopeChange }: ScopeBarProps) {
  const { t } = useTranslation();
  // Spec list comes from the standard reader; we filter to active wave-plan
  // parents + standalone active specs so the dropdown stays short. A spec
  // with no recorded waves is still listed under "Spec" — only the "Wave"
  // dropdown needs the children.
  const specs = useQuery({
    queryKey: ["specs", projectPath],
    queryFn: () => fetchSpecs(projectPath),
    enabled: !!projectPath,
    staleTime: 30_000,
  });

  const projects = useProjects();

  const activeSpecs = useMemo(
    () => (specs.data ?? []).filter((s) => s.bucket === "active" && !s.parent),
    [specs.data],
  );

  // Wave children for the currently picked spec — empty list when the user
  // hasn't selected a spec yet under the Wave tab.
  const selectedSpecForWave =
    scope.kind === "wave" ? scope.spec : scope.kind === "spec" ? scope.spec : "";
  const waveChildren: SpecRow[] = useMemo(
    () => (specs.data ?? []).filter((s) => s.parent === selectedSpecForWave),
    [specs.data, selectedSpecForWave],
  );

  // Local state for the multi-project picker. Initialised from the active
  // workspace so first render has at least one entry — the user can extend.
  const [comparePicks, setComparePicks] = useState<string[]>(() => {
    if (scope.kind === "all_projects") return scope.projects;
    return projectPath ? [projectPath] : [];
  });

  // Keep `comparePicks` in sync when the parent forces a scope reset.
  useEffect(() => {
    if (scope.kind === "all_projects") setComparePicks(scope.projects);
  }, [scope]);

  function switchTo(kind: EconomyScopeKind) {
    if (kind === "project") {
      onScopeChange(projectScope(projectPath));
      return;
    }
    if (kind === "spec") {
      // Default to the first active spec if any; otherwise leave the picker
      // empty and let the user choose.
      const first = activeSpecs[0]?.name ?? "";
      onScopeChange(specScope(projectPath, first));
      return;
    }
    if (kind === "wave") {
      const firstSpec = activeSpecs[0]?.name ?? "";
      const firstWave =
        (specs.data ?? []).find((s) => s.parent === firstSpec)?.name ?? "";
      onScopeChange(waveScope(projectPath, firstSpec, firstWave));
      return;
    }
    // all_projects
    const picks = comparePicks.length > 0 ? comparePicks : projects.map((p) => p.path);
    onScopeChange(allProjectsScope(picks));
  }

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-1.5">
        {TAB_META.map((tab) => {
          const active = scope.kind === tab.kind;
          return (
            <button
              key={tab.kind}
              type="button"
              onClick={() => switchTo(tab.kind)}
              className={cn(
                "inline-flex items-center gap-1.5 px-3 py-1.5 rounded-[--ds-radius-md] text-[12px] font-medium transition-colors",
                "border",
                active
                  ? "bg-[--ds-accent-primary]/10 border-[--ds-accent-primary]/40 text-[--ds-accent-primary]"
                  : "bg-[--ds-surface-base] border-[--ds-surface-hover] text-[--ds-text-secondary] hover:text-[--ds-text-primary] hover:bg-[--ds-surface-hover]",
              )}
            >
              {tab.icon}
              <span>{t(tab.labelKey)}</span>
            </button>
          );
        })}
      </div>

      {/* Spec dropdown — visible when Spec tab is active. */}
      {scope.kind === "spec" && (
        <div className="flex items-center gap-2 text-[12px]">
          <label className="text-[--ds-text-tertiary]">{t("economy.scope.specLabel")}</label>
          <select
            value={scope.spec}
            onChange={(e) => onScopeChange(specScope(projectPath, e.target.value))}
            className="bg-[--ds-surface-base] border border-[--ds-surface-hover] rounded-[--ds-radius-sm] px-2 py-1 text-[--ds-text-primary] min-w-[260px]"
          >
            <option value="">{t("economy.scope.selectPlaceholder")}</option>
            {activeSpecs.map((s) => (
              <option key={s.name} value={s.name}>
                {s.name}
              </option>
            ))}
          </select>
        </div>
      )}

      {/* Wave: cascading spec → wave dropdowns. */}
      {scope.kind === "wave" && (
        <div className="flex items-center gap-2 text-[12px] flex-wrap">
          <label className="text-[--ds-text-tertiary]">{t("economy.scope.specLabel")}</label>
          <select
            value={scope.spec}
            onChange={(e) => onScopeChange(waveScope(projectPath, e.target.value, ""))}
            className="bg-[--ds-surface-base] border border-[--ds-surface-hover] rounded-[--ds-radius-sm] px-2 py-1 text-[--ds-text-primary] min-w-[220px]"
          >
            <option value="">{t("economy.scope.selectPlaceholder")}</option>
            {activeSpecs.map((s) => (
              <option key={s.name} value={s.name}>
                {s.name}
              </option>
            ))}
          </select>
          <label className="text-[--ds-text-tertiary]">{t("economy.scope.waveLabel")}</label>
          <select
            value={scope.wave}
            onChange={(e) =>
              onScopeChange(waveScope(projectPath, scope.spec, e.target.value))
            }
            disabled={!scope.spec || waveChildren.length === 0}
            className="bg-[--ds-surface-base] border border-[--ds-surface-hover] rounded-[--ds-radius-sm] px-2 py-1 text-[--ds-text-primary] min-w-[220px] disabled:opacity-40"
          >
            <option value="">{t("economy.scope.selectPlaceholder")}</option>
            {waveChildren.map((w) => (
              <option key={w.name} value={w.name}>
                {w.name}
              </option>
            ))}
          </select>
        </div>
      )}

      {/* Compare projects: multi-select via checkboxes. */}
      {scope.kind === "all_projects" && (
        <div className="flex flex-col gap-1.5 text-[12px]">
          <label className="text-[--ds-text-tertiary]">{t("economy.scope.compareLabel")}</label>
          <div className="flex flex-wrap gap-2">
            {projects.length === 0 ? (
              <span className="text-[--ds-text-tertiary] italic">
                {t("economy.scope.noProjects")}
              </span>
            ) : (
              projects.map((p) => {
                const checked = comparePicks.includes(p.path);
                return (
                  <label
                    key={p.id}
                    className={cn(
                      "inline-flex items-center gap-1.5 px-2 py-1 rounded-[--ds-radius-sm] border cursor-pointer transition-colors",
                      checked
                        ? "bg-[--ds-accent-primary]/10 border-[--ds-accent-primary]/40 text-[--ds-accent-primary]"
                        : "bg-[--ds-surface-base] border-[--ds-surface-hover] text-[--ds-text-secondary] hover:bg-[--ds-surface-hover]",
                    )}
                  >
                    <input
                      type="checkbox"
                      checked={checked}
                      onChange={(e) => {
                        const next = e.target.checked
                          ? Array.from(new Set([...comparePicks, p.path]))
                          : comparePicks.filter((x) => x !== p.path);
                        setComparePicks(next);
                        onScopeChange(allProjectsScope(next));
                      }}
                      className="sr-only"
                    />
                    <span className="truncate max-w-[200px]">{p.name}</span>
                  </label>
                );
              })
            )}
          </div>
        </div>
      )}
    </div>
  );
}
