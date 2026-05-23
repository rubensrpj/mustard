import { useMemo, useState } from 'react';
import { X } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { Badge } from '@/components/ui/badge';
import { ScrollArea } from '@/components/ui/scroll-area';

/**
 * Multi-select picker for entity-registry entries. Renders selected entities
 * as removable chips above a scrollable, searchable checkbox list. Entities
 * marked by the AI as part of the scope (`prePicked`) get a subtle "sugerida"
 * tag so the user can see what the lapidator inferred without losing the
 * ability to add/remove freely.
 */
export interface EntityPickerProps {
  /** Full entity universe from the project's `.claude/entity-registry.json`. */
  entities: string[];
  selected: string[];
  onChange: (sel: string[]) => void;
  /** Entities the AI flagged as in-scope (rendered with a subtle indicator). */
  prePicked?: string[];
}

export function EntityPicker({
  entities,
  selected,
  onChange,
  prePicked = [],
}: EntityPickerProps) {
  const [query, setQuery] = useState('');

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return entities;
    return entities.filter((e) => e.toLowerCase().includes(q));
  }, [entities, query]);

  function toggle(name: string, checked: boolean) {
    if (checked) {
      if (!selected.includes(name)) onChange([...selected, name]);
    } else {
      onChange(selected.filter((s) => s !== name));
    }
  }

  if (entities.length === 0) {
    return (
      <p className="text-xs text-muted-foreground border border-dashed border-border rounded px-3 py-2">
        Nenhuma entidade no registry. Rode <code className="font-mono">/mustard:sync-registry</code> no projeto para popular.
      </p>
    );
  }

  const preSet = new Set(prePicked);

  return (
    <div className="flex flex-col gap-2">
      {selected.length > 0 && (
        <div className="flex flex-wrap gap-1">
          {selected.map((name) => (
            <Badge
              key={name}
              variant="tag-purple"
              className="cursor-pointer"
              onClick={() => toggle(name, false)}
            >
              {name}
              <X className="h-3 w-3" />
            </Badge>
          ))}
        </div>
      )}
      <Input
        placeholder="Filtrar entidades…"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        aria-label="Filtrar entidades"
      />
      <ScrollArea className="border border-border rounded max-h-48 overflow-auto">
        <div className="flex flex-col p-1.5">
          {filtered.length === 0 ? (
            <span className="text-xs text-muted-foreground px-2 py-1">Sem resultados</span>
          ) : (
            filtered.map((name) => {
              const isSelected = selected.includes(name);
              const isPre = preSet.has(name);
              return (
                <label
                  key={name}
                  className="flex items-center gap-2 text-sm px-2 py-1 rounded hover:bg-muted cursor-pointer"
                >
                  <input
                    type="checkbox"
                    checked={isSelected}
                    onChange={(e) => toggle(name, e.target.checked)}
                    className="rounded"
                  />
                  <span className="flex-1 truncate">{name}</span>
                  {isPre && (
                    <span className="text-[10px] uppercase tracking-wider text-primary/70">
                      sugerida
                    </span>
                  )}
                </label>
              );
            })
          )}
        </div>
      </ScrollArea>
    </div>
  );
}
