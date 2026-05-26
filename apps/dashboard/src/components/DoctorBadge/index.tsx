// DoctorBadge — sidebar-footer installation-health indicator (W10.T10.8).
//
// Renders one of three coloured dots:
//   - green  (ok)   — every doctor check passed
//   - yellow (warn) — at least one WARN (drift, missing nerd-font, etc.)
//   - red    (fail) — at least one FAIL (broken wiring, parse error, etc.)
//
// On hover, the tooltip lists each failing/warning check with its message and
// a fix-command hint ("run `mustard-rt run doctor --residue`"). The badge is
// scoped to the active workspace project — when no project is registered or
// active, nothing renders (`null`).
//
// Data flow: TanStack Query fetches the doctor report via the `doctor_status`
// Tauri command (`@/lib/doctor`). Polling is `staleTime: 30s` — the typical
// developer ergonomics window for "did I forget to install something?".

import { useQuery } from "@tanstack/react-query";
import { Activity } from "lucide-react";

import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { doctorStatus, type DoctorStatus, type DoctorOverall } from "@/lib/doctor";

interface DoctorBadgeProps {
  /** Absolute path of the active project. Required — the badge only renders
   *  when a workspace is selected. */
  projectPath: string | null | undefined;
}

const DOT_BY_OVERALL: Record<DoctorOverall, string> = {
  ok: "bg-[--intent-success] ring-[--intent-success]/30",
  warn: "bg-amber-400 ring-amber-400/30",
  fail: "bg-[--intent-error] ring-[--intent-error]/30",
};

const LABEL_BY_OVERALL: Record<DoctorOverall, string> = {
  ok: "Doctor: healthy",
  warn: "Doctor: warnings",
  fail: "Doctor: failures",
};

/// Map a check name to a fix-command hint. Keeps the tooltip actionable rather
/// than just listing problems — the user reads "wave-integrity warn" and sees
/// `mustard-rt run doctor --check wave-integrity` underneath.
function fixHintFor(checkName: string): string | null {
  switch (checkName) {
    case "wiring":
      return "Edit `.claude/settings.json` — referenced events/subcommands missing.";
    case "drift":
      return "Run `mustard update` to resync the `.claude/` payload.";
    case "state-health":
      return "Run `mustard-rt run sync-registry` (entity-registry missing).";
    case "claude_cli":
      return "Install Claude Code and add `claude` to PATH.";
    case "lsp":
      return "Install the language-server binary listed in details.";
    case "nerd-font":
      return "Run `mustard install-nerd-font` (statusline tofu fix).";
    case "wave-integrity":
      return "A `[[wave-N-role]]` link points to a missing dir — regenerate via `mustard-rt run wave-scaffold`.";
    case "skill-discovery":
      return "Edit SKILL.md frontmatter (`description` clauses below threshold).";
    case "residue":
      return "Remove dead `.js` references in `.claude/settings.json`.";
    default:
      return null;
  }
}

export function DoctorBadge({ projectPath }: DoctorBadgeProps) {
  const enabled = Boolean(projectPath);
  const { data, isLoading, error } = useQuery<DoctorStatus>({
    queryKey: ["doctor-status", projectPath ?? ""],
    queryFn: () => doctorStatus(projectPath as string),
    enabled,
    staleTime: 30_000,
    retry: false,
  });

  if (!enabled) return null;

  // Pending state: render a neutral pulse so the sidebar footer doesn't shift
  // height once the query resolves. Failure-of-the-query (network etc.) is
  // surfaced as red — we can't claim healthy if we couldn't ask.
  let overall: DoctorOverall = "warn";
  let tooltipBody: React.ReactNode = "Checking…";

  if (error) {
    overall = "fail";
    tooltipBody = "Could not run mustard-rt doctor.";
  } else if (data) {
    overall = data.overall;
    if (data.error) {
      tooltipBody = data.error;
    } else {
      // Only surface checks that are not OK or SKIP to keep the tooltip tight.
      const problems = data.checks.filter(
        (c) => c.status === "warn" || c.status === "fail",
      );
      tooltipBody = problems.length === 0 ? (
        <span>All checks passing.</span>
      ) : (
        <ul className="flex flex-col gap-1.5 text-xs">
          {problems.map((c) => {
            const hint = fixHintFor(c.name);
            const tone =
              c.status === "fail"
                ? "text-[--intent-error]"
                : "text-amber-400";
            return (
              <li key={c.name} className="flex flex-col gap-0.5">
                <span className={cn("font-medium", tone)}>
                  {c.name} — {c.status}
                </span>
                {c.message && (
                  <span className="text-muted-foreground">{c.message}</span>
                )}
                {hint && (
                  <span className="text-muted-foreground italic">{hint}</span>
                )}
              </li>
            );
          })}
        </ul>
      );
    }
  } else if (isLoading) {
    tooltipBody = "Checking…";
  }

  const dotClass = DOT_BY_OVERALL[overall];
  const ariaLabel = LABEL_BY_OVERALL[overall];

  return (
    <TooltipProvider delayDuration={200}>
      <Tooltip>
        <TooltipTrigger asChild>
          <button
            type="button"
            aria-label={ariaLabel}
            className={cn(
              "flex items-center gap-2 px-3 py-1.5 rounded-md text-sm transition-colors duration-150",
              "text-sidebar-foreground/80 hover:bg-muted/40 hover:text-foreground",
            )}
          >
            <Activity className="h-3.5 w-3.5" />
            <span className="flex-1 text-left">Doctor</span>
            <span
              aria-hidden
              className={cn(
                "w-2 h-2 rounded-full ring-1 shrink-0",
                dotClass,
              )}
            />
          </button>
        </TooltipTrigger>
        <TooltipContent side="top" align="end" className="max-w-xs">
          {tooltipBody}
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}
