import { useEffect, useMemo, useState } from "react";
import { Search, ChevronRight, ChevronDown, Copy } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { COMMANDS, CATEGORIES } from "@/data/commands-catalog";

function slug(cmd: string): string {
  return cmd.replace("/mustard:", "").replace(":", "-");
}

export function Commands() {
  const [query, setQuery] = useState("");
  const [debouncedQuery, setDebouncedQuery] = useState("");
  const [selectedCategory, setSelectedCategory] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());

  useEffect(() => {
    const t = setTimeout(() => setDebouncedQuery(query), 300);
    return () => clearTimeout(t);
  }, [query]);

  useEffect(() => {
    function onHashChange() {
      const hash = window.location.hash.slice(1);
      if (!hash) return;
      const el = document.getElementById(hash);
      if (!el) return;
      el.scrollIntoView({ behavior: "smooth", block: "start" });
      const cmdSlug = hash.startsWith("cmd-") ? hash.slice(4) : hash;
      setExpanded((prev) => {
        if (prev.has(cmdSlug)) return prev;
        const next = new Set(prev);
        next.add(cmdSlug);
        return next;
      });
    }
    onHashChange();
    window.addEventListener("hashchange", onHashChange);
    return () => window.removeEventListener("hashchange", onHashChange);
  }, []);

  const filtered = useMemo(() => {
    let list = COMMANDS;
    if (selectedCategory !== null) {
      list = list.filter((c) => c.category === selectedCategory);
    }
    const q = debouncedQuery.trim().toLowerCase();
    if (q.length >= 2) {
      list = list.filter((c) =>
        (c.cmd + " " + c.simples + " " + c.tecnico + " " + c.short)
          .toLowerCase()
          .includes(q)
      );
    }
    return list;
  }, [debouncedQuery, selectedCategory]);

  function toggle(s: string) {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(s)) next.delete(s);
      else next.add(s);
      return next;
    });
  }

  function scrollToSlug(s: string) {
    window.location.hash = "cmd-" + s;
    document.getElementById("cmd-" + s)?.scrollIntoView({ behavior: "smooth", block: "start" });
  }

  const trimmed = debouncedQuery.trim();

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-1">
        <nav className="text-[13px] text-muted-foreground">
          Mustard / <span className="text-foreground">Comandos</span>
        </nav>
        <div className="flex items-baseline gap-2">
          <h1 className="text-base font-medium">Catálogo de comandos</h1>
          <span className="font-mono text-muted-foreground/50 text-[13px]">({COMMANDS.length})</span>
        </div>
      </div>

      <div className="relative">
        <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground" />
        <input
          autoFocus
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Buscar por nome, descrição, categoria…"
          className="w-full pl-9 pr-3 py-2 bg-card border border-border rounded text-sm outline-none placeholder:text-muted-foreground focus:border-primary"
        />
      </div>

      <div className="flex flex-wrap gap-1.5">
        <button
          type="button"
          onClick={() => setSelectedCategory(null)}
          className={`px-2.5 py-0.5 rounded text-[12px] border transition-colors ${selectedCategory === null ? "bg-primary/10 text-primary border-primary/30" : "border-border text-muted-foreground hover:text-foreground hover:border-border/80"}`}
        >
          Todos
        </button>
        {CATEGORIES.map((cat) => (
          <button
            key={cat}
            type="button"
            onClick={() => setSelectedCategory(selectedCategory === cat ? null : cat)}
            className={`px-2.5 py-0.5 rounded text-[12px] border transition-colors ${selectedCategory === cat ? "bg-primary/10 text-primary border-primary/30" : "border-border text-muted-foreground hover:text-foreground hover:border-border/80"}`}
          >
            {cat}
          </button>
        ))}
      </div>

      {filtered.length === 0 ? (
        <p className="text-[13px] text-muted-foreground">
          {COMMANDS.length === 0
            ? "Nenhum comando catalogado."
            : `Nenhum comando para "${trimmed || selectedCategory}".`}
        </p>
      ) : (
        <ul className="flex flex-col gap-0.5 text-sm">
          {filtered.map((entry) => {
            const s = slug(entry.cmd);
            const isOpen = expanded.has(s);
            const Chevron = isOpen ? ChevronDown : ChevronRight;
            return (
              <li
                key={entry.cmd}
                id={`cmd-${s}`}
                className="flex flex-col rounded border border-transparent hover:border-border/40 hover:bg-muted/20"
              >
                <button
                  type="button"
                  onClick={() => toggle(s)}
                  className="flex items-center gap-2.5 px-3 py-2 text-left w-full cursor-pointer"
                >
                  <Chevron className="h-3.5 w-3.5 text-muted-foreground shrink-0" />
                  <span className="font-mono text-[13px] text-foreground shrink-0">{entry.cmd}</span>
                  <Badge variant="secondary" className="text-[11px] py-0 shrink-0">{entry.category}</Badge>
                  <span className="text-muted-foreground text-[13px] truncate">{entry.short}</span>
                </button>

                {isOpen && (
                  <div className="px-4 pb-4 pt-1 flex flex-col gap-3">
                    <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                      <div className="flex flex-col gap-1">
                        <span className="text-[11px] uppercase tracking-wider text-muted-foreground">Explicação simples</span>
                        <p className="text-[13px]">{entry.simples}</p>
                      </div>
                      <div className="flex flex-col gap-1">
                        <span className="text-[11px] uppercase tracking-wider text-muted-foreground">Detalhes técnicos</span>
                        <p className="text-[13px] text-muted-foreground">{entry.tecnico}</p>
                      </div>
                    </div>

                    <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                      <div className="flex flex-col gap-1">
                        <span className="text-[11px] uppercase tracking-wider text-muted-foreground">Quando usar</span>
                        <p className="text-[13px]">{entry.when}</p>
                      </div>
                      <div className="flex flex-col gap-1">
                        <span className="text-[11px] uppercase tracking-wider text-muted-foreground">Quando NÃO usar</span>
                        <p className="text-[13px] text-muted-foreground">{entry.notWhen}</p>
                      </div>
                    </div>

                    {entry.examples.length > 0 && (
                      <div className="flex flex-col gap-1">
                        <span className="text-[11px] uppercase tracking-wider text-muted-foreground">Exemplos</span>
                        <div className="flex flex-col gap-1">
                          {entry.examples.map((ex) => (
                            <div key={ex} className="flex items-center gap-2">
                              <code className="font-mono text-[12px] bg-muted/40 px-2 py-1 rounded flex-1">{ex}</code>
                              <button
                                type="button"
                                onClick={() => navigator.clipboard.writeText(ex)}
                                className="p-1 rounded text-muted-foreground hover:text-foreground hover:bg-muted/40 transition-colors"
                                title="Copiar"
                              >
                                <Copy className="h-3.5 w-3.5" />
                              </button>
                            </div>
                          ))}
                        </div>
                      </div>
                    )}

                    {entry.seeAlso.length > 0 && (
                      <div className="flex flex-col gap-1">
                        <span className="text-[11px] uppercase tracking-wider text-muted-foreground">Ver também</span>
                        <div className="flex flex-wrap gap-1.5">
                          {entry.seeAlso.map((rel) => (
                            <button
                              key={rel}
                              type="button"
                              onClick={() => scrollToSlug(rel)}
                              className="inline-flex"
                            >
                              <Badge variant="secondary" className="text-[11px] cursor-pointer hover:bg-primary/10 hover:text-primary transition-colors">
                                /{rel}
                              </Badge>
                            </button>
                          ))}
                        </div>
                      </div>
                    )}
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
