import { useEffect, useMemo, useState } from "react";
import { Search, ChevronRight, ChevronDown, Copy } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import {
  PageSurface,
  EditorialBand,
  DataCard,
  EmptyState,
} from "@/components/page";
import { COMMANDS, CATEGORIES } from "@/data/commands-catalog";
import { useT } from "@/lib/i18n";

function slug(cmd: string): string {
  return cmd.replace("/mustard:", "").replace(":", "-");
}

export function Commands() {
  const t = useT();
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
    <PageSurface>
      <EditorialBand
        eyebrow="Mustard"
        title={t("commands.editorialTitle")}
        subtitle={t("commands.editorialSubtitle")}
      />

      <div className="relative">
        <Search
          className="absolute left-3 inset-y-0 my-auto h-3.5 w-3.5 text-muted-foreground"
          aria-hidden
        />
        <input
          autoFocus
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={t("commands.search.placeholder")}
          className="w-full pl-9 pr-3 py-2 bg-card border border-border rounded text-sm outline-none placeholder:text-muted-foreground focus:border-primary"
        />
      </div>

      <div className="flex flex-wrap gap-1.5">
        <button
          type="button"
          onClick={() => setSelectedCategory(null)}
          className={`px-2.5 py-0.5 rounded text-[12px] border transition-colors ${selectedCategory === null ? "bg-primary text-primary-foreground border-primary" : "border-border text-muted-foreground hover:text-foreground"}`}
        >
          {t("commands.filter.all")}
        </button>
        {CATEGORIES.map((cat) => (
          <button
            key={cat}
            type="button"
            onClick={() => setSelectedCategory(selectedCategory === cat ? null : cat)}
            className={`px-2.5 py-0.5 rounded text-[12px] border transition-colors ${selectedCategory === cat ? "bg-primary text-primary-foreground border-primary" : "border-border text-muted-foreground hover:text-foreground"}`}
          >
            {cat}
          </button>
        ))}
      </div>

      {filtered.length === 0 ? (
        <EmptyState
          title={COMMANDS.length === 0 ? t("commands.empty.noCatalog.title") : t("commands.empty.noResults.title")}
          description={
            COMMANDS.length === 0
              ? t("commands.empty.noCatalog.description")
              : t("commands.empty.noResults.description").replace("{query}", trimmed || selectedCategory || "")
          }
        />
      ) : (
        <DataCard>
          {filtered.map((entry) => {
            const s = slug(entry.cmd);
            const isOpen = expanded.has(s);
            const Chevron = isOpen ? ChevronDown : ChevronRight;
            return (
              <div
                key={entry.cmd}
                id={`cmd-${s}`}
                className="flex flex-col border-b border-border"
              >
                <button
                  type="button"
                  onClick={() => toggle(s)}
                  className="flex items-center gap-2.5 px-3 py-2 text-left w-full cursor-pointer hover:bg-card"
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
                        <span className="text-[11px] uppercase tracking-wider text-muted-foreground">{t("commands.section.plainExplanation")}</span>
                        <p className="text-[13px]">{entry.simples}</p>
                      </div>
                      <div className="flex flex-col gap-1">
                        <span className="text-[11px] uppercase tracking-wider text-muted-foreground">{t("commands.section.technicalDetails")}</span>
                        <p className="text-[13px] text-muted-foreground">{entry.tecnico}</p>
                      </div>
                    </div>

                    <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                      <div className="flex flex-col gap-1">
                        <span className="text-[11px] uppercase tracking-wider text-muted-foreground">{t("commands.section.whenToUse")}</span>
                        <p className="text-[13px]">{entry.when}</p>
                      </div>
                      <div className="flex flex-col gap-1">
                        <span className="text-[11px] uppercase tracking-wider text-muted-foreground">{t("commands.section.whenNotToUse")}</span>
                        <p className="text-[13px] text-muted-foreground">{entry.notWhen}</p>
                      </div>
                    </div>

                    {entry.examples.length > 0 && (
                      <div className="flex flex-col gap-1">
                        <span className="text-[11px] uppercase tracking-wider text-muted-foreground">{t("commands.section.examples")}</span>
                        <div className="flex flex-col gap-1">
                          {entry.examples.map((ex) => (
                            <div key={ex} className="flex items-center gap-2">
                              <code className="font-mono text-[12px] bg-muted px-2 py-1 rounded flex-1">{ex}</code>
                              <button
                                type="button"
                                onClick={() => navigator.clipboard.writeText(ex)}
                                className="p-1 rounded text-muted-foreground hover:text-foreground hover:bg-muted transition-colors"
                                title={t("commands.copy")}
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
                        <span className="text-[11px] uppercase tracking-wider text-muted-foreground">{t("commands.section.seeAlso")}</span>
                        <div className="flex flex-wrap gap-1.5">
                          {entry.seeAlso.map((rel) => (
                            <button
                              key={rel}
                              type="button"
                              onClick={() => scrollToSlug(rel)}
                              className="inline-flex"
                            >
                              <Badge variant="secondary" className="text-[11px] cursor-pointer hover:bg-primary hover:text-primary-foreground transition-colors">
                                /{rel}
                              </Badge>
                            </button>
                          ))}
                        </div>
                      </div>
                    )}
                  </div>
                )}
              </div>
            );
          })}
        </DataCard>
      )}
    </PageSurface>
  );
}
