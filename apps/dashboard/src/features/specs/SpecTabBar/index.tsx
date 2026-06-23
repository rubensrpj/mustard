import { List, Plus, RefreshCw, X } from "lucide-react";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";

export type SpecTab =
  | { id: "list"; kind: "list" }
  | {
      id: string;
      kind: "spec";
      specName: string;
      /** Wave to pre-select when the tab first mounts (set when the tab was
       *  opened by clicking a wave-child in the list tree). Optional — opening
       *  a spec normally leaves no wave selected. */
      initialWave?: number;
      /** Monotonic nonce bumped on each wave-bearing open, so re-clicking the
       *  SAME wave-child still re-selects it (the detail effect keys on this,
       *  not on the wave value, which would no-op on a same-wave re-click). */
      initialWaveNonce?: number;
    };

interface SpecTabBarProps {
  tabs: SpecTab[];
  activeId: string;
  onActivate: (id: string) => void;
  onClose: (id: string) => void;
  onAddRequest: () => void;
  onRefresh: () => void;
  className?: string;
}

/**
 * Horizontal tab bar at the top of the Specs route. The "Lista" tab is
 * always present (no close `×`); spec tabs render the slug truncated with
 * a hover-revealed `×`. Trailing `+` opens quick-open and `⟳` refetches the
 * active tab's queries. Overflows horizontally with `overflow-x-auto`.
 */
export function SpecTabBar({
  tabs,
  activeId,
  onActivate,
  onClose,
  onAddRequest,
  onRefresh,
  className,
}: SpecTabBarProps) {
  return (
    <div
      className={cn(
        "flex items-center gap-1 border-b border-border bg-muted/40 px-1.5 py-1",
        className,
      )}
      role="tablist"
      aria-label="Abas de specs"
    >
      <div className="flex items-center gap-0.5 flex-1 min-w-0 overflow-x-auto">
        {tabs.map((tab) => {
          const isActive = tab.id === activeId;
          const isList = tab.kind === "list";
          const label = isList ? "Lista" : tab.specName;
          return (
            <div
              key={tab.id}
              className={cn(
                "group/spec-tab relative flex items-center gap-1.5 rounded-md px-2 py-1 text-[12px] cursor-pointer select-none shrink-0 max-w-[260px]",
                "transition-colors duration-100",
                isActive
                  ? "bg-background text-foreground shadow-sm"
                  : "text-muted-foreground hover:bg-muted hover:text-foreground",
              )}
              role="tab"
              aria-selected={isActive}
              tabIndex={isActive ? 0 : -1}
              onClick={() => onActivate(tab.id)}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  onActivate(tab.id);
                }
              }}
              title={isList ? "Lista de specs" : label}
            >
              {isList ? (
                <List className="h-3.5 w-3.5 shrink-0" aria-hidden />
              ) : null}
              <span
                className={cn(
                  "truncate font-mono tabular-nums",
                  isList && "font-sans",
                )}
                style={isList ? undefined : { fontVariantNumeric: "tabular-nums" }}
              >
                {label}
              </span>
              {!isList && (
                <button
                  type="button"
                  onClick={(e) => {
                    e.stopPropagation();
                    onClose(tab.id);
                  }}
                  aria-label={`Fechar aba ${label}`}
                  title="Fechar aba"
                  className={cn(
                    "h-4 w-4 flex items-center justify-center rounded shrink-0",
                    "text-muted-foreground hover:text-foreground hover:bg-muted/80",
                    isActive
                      ? "opacity-70 hover:opacity-100"
                      : "opacity-0 group-hover/spec-tab:opacity-100 focus-visible:opacity-100",
                    "transition-opacity",
                  )}
                >
                  <X className="h-3 w-3" aria-hidden />
                </button>
              )}
            </div>
          );
        })}
      </div>

      <div className="flex items-center gap-0.5 shrink-0 pl-1">
        <Button
          variant="ghost"
          size="icon-xs"
          onClick={onAddRequest}
          aria-label="Abrir spec em nova aba"
          title="Abrir spec em nova aba (quick-open)"
        >
          <Plus className="h-3.5 w-3.5" aria-hidden />
        </Button>
        <Button
          variant="ghost"
          size="icon-xs"
          onClick={onRefresh}
          aria-label="Atualizar dados da aba ativa"
          title="Atualizar dados da aba ativa"
        >
          <RefreshCw className="h-3.5 w-3.5" aria-hidden />
        </Button>
      </div>
    </div>
  );
}
