import { useMemo, useState } from "react";
import { useNavigate } from "react-router";
import { ChevronDown, Settings as SettingsIcon } from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { StatusDot, type StatusDotVariant } from "@/components/StatusDot";
import { cn } from "@/lib/utils";
import { relativeTime } from "@/lib/time";
import type { Project } from "@/api/discovery";

interface WorkspaceSwitcherProps {
  projects: Project[];
  activeId: string | null;
  onSelect: (id: string) => void;
  projectsRoot: string | null;
  loading?: boolean;
}

function statusFor(ms: number | null): StatusDotVariant {
  if (ms === null) return "idle";
  const delta = Date.now() - ms;
  if (delta < 3_600_000) return "active";
  if (delta < 86_400_000) return "idle";
  return "blocked"; // stale
}

function initials(name: string): string {
  const parts = name.trim().split(/\s+/).slice(0, 2);
  const chars = parts.map((p) => p[0]).join("");
  return (chars || name.slice(0, 2) || "?").toUpperCase();
}

export function WorkspaceSwitcher({
  projects,
  activeId,
  onSelect,
  projectsRoot,
  loading,
}: WorkspaceSwitcherProps) {
  const navigate = useNavigate();
  const [open, setOpen] = useState(false);
  const active = useMemo(
    () => projects.find((p) => p.id === activeId) ?? null,
    [projects, activeId],
  );
  const disabled = !projectsRoot;

  const trigger = (
    <DropdownMenuTrigger asChild disabled={disabled}>
      <button
        type="button"
        className={cn(
          "inline-flex items-center justify-between gap-2 w-full px-3 py-2 bg-sidebar text-sm transition-colors",
          "hover:bg-muted/40",
          disabled && "opacity-50 cursor-not-allowed",
        )}
      >
        <Avatar className="size-5">
          <AvatarFallback className="text-[10px] font-medium">
            {active ? initials(active.name) : "—"}
          </AvatarFallback>
        </Avatar>
        <span className="flex-1 text-left truncate font-medium">
          {active ? active.name : loading ? "Carregando…" : "Selecionar workspace"}
        </span>
        <ChevronDown className="h-3.5 w-3.5 text-muted-foreground shrink-0" />
      </button>
    </DropdownMenuTrigger>
  );

  return (
    <DropdownMenu open={open} onOpenChange={setOpen}>
      {disabled ? (
        <TooltipProvider>
          <Tooltip>
            <TooltipTrigger asChild>
              <span>{trigger}</span>
            </TooltipTrigger>
            <TooltipContent side="bottom">Configure root em Settings</TooltipContent>
          </Tooltip>
        </TooltipProvider>
      ) : (
        trigger
      )}
      <DropdownMenuContent align="start" className="w-[280px] p-0">
        <Command>
          <CommandInput placeholder="Buscar workspace…" className="h-9" />
          <CommandList>
            <CommandEmpty>
              {projects.length === 0
                ? "Nenhum workspace encontrado — configure root em Settings"
                : "Sem resultados."}
            </CommandEmpty>
            {projects.length > 0 && (
              <CommandGroup>
                {projects.map((p) => {
                  const variant = statusFor(p.last_activity_ms);
                  return (
                    <CommandItem
                      key={p.id}
                      value={p.name}
                      onSelect={() => {
                        onSelect(p.id);
                        setOpen(false);
                      }}
                      className="flex items-center gap-2"
                    >
                      <StatusDot variant={variant} size="md" />
                      <span className="flex-1 truncate">{p.name}</span>
                      <span className="text-[11px] text-muted-foreground">
                        {p.last_activity_ms
                          ? relativeTime(new Date(p.last_activity_ms).toISOString())
                          : "—"}
                      </span>
                    </CommandItem>
                  );
                })}
              </CommandGroup>
            )}
          </CommandList>
        </Command>
        <DropdownMenuSeparator />
        <DropdownMenuItem
          onSelect={() => {
            setOpen(false);
            navigate("/settings");
          }}
        >
          <SettingsIcon className="h-3.5 w-3.5" />
          Abrir Settings
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
