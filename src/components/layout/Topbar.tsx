import { useLocation } from "react-router";
import { Sun, Moon, RefreshCw } from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";
import { useTheme } from "@/hooks/useTheme";
import { useStore } from "@/lib/store";

function detectModKey(): "Ctrl" | "⌘" {
  if (typeof navigator === "undefined") return "Ctrl";
  return /Mac|iPhone|iPad/.test(navigator.platform) ? "⌘" : "Ctrl";
}

function pathLabel(pathname: string): string {
  if (pathname === "/") return "Home";
  if (pathname === "/settings") return "Settings";
  if (pathname.startsWith("/project/")) return "Projeto";
  return pathname.replace(/^\//, "");
}

export function Topbar() {
  const location = useLocation();
  const label = pathLabel(location.pathname);
  const { theme, toggle } = useTheme();
  const modKey = detectModKey();
  const queryClient = useQueryClient();
  const projectsRoot = useStore((s) => s.projectsRoot);

  return (
    <header className="row-start-1 col-start-2 h-12 sticky top-0 bg-background border-b border-border flex items-center justify-between px-5 z-10">
      <nav className="text-sm flex items-center gap-1.5" aria-label="Breadcrumb">
        <span className="text-muted-foreground">Mustard</span>
        <span className="text-muted-foreground/40">/</span>
        <span className="text-foreground font-medium">{label}</span>
      </nav>
      <div className="flex items-center gap-1.5">
        <kbd className="hidden sm:inline-flex items-center gap-1 px-1.5 py-0.5 rounded border border-border bg-muted/40 text-[10px] text-muted-foreground font-mono">
          <span>{modKey}</span>
          <span>K</span>
        </kbd>
        <button
          type="button"
          title="Reload projects"
          aria-label="Reload projects"
          disabled={!projectsRoot}
          onClick={() => {
            if (projectsRoot) {
              queryClient.invalidateQueries({ queryKey: ['discover', projectsRoot] });
            }
          }}
          className={`h-7 w-7 rounded-md text-muted-foreground hover:bg-muted hover:text-foreground transition-colors duration-150 inline-flex items-center justify-center ${!projectsRoot ? "opacity-50 cursor-not-allowed" : ""}`}
        >
          <RefreshCw className="h-3.5 w-3.5" />
        </button>
        <button
          type="button"
          onClick={toggle}
          aria-label={theme === "dark" ? "Switch to light theme" : "Switch to dark theme"}
          className="inline-flex items-center justify-center h-7 w-7 rounded-md text-muted-foreground hover:bg-muted hover:text-foreground transition-colors duration-150"
        >
          {theme === "dark" ? <Sun className="h-3.5 w-3.5" /> : <Moon className="h-3.5 w-3.5" />}
        </button>
      </div>
    </header>
  );
}
