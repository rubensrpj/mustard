import { useRef, type KeyboardEvent } from 'react';
import { Plus, X } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';

/**
 * Edits an array of strings as one input-per-row, with add/remove buttons.
 * Replaces `<textarea value={items.join("\n")} />` for boundaries/checklist
 * so users get tactile add/remove instead of free-form typing.
 *
 * Empty state: renders a single placeholder row so the user always has an
 * input to type into without first hitting "+ Adicionar".
 */
export interface EditableListProps {
  items: string[];
  onChange: (items: string[]) => void;
  placeholder?: string;
  addLabel?: string;
  /** Optional aria-label prefix; rendered as `${prefix} ${i+1}`. */
  itemAriaPrefix?: string;
}

export function EditableList({
  items,
  onChange,
  placeholder,
  addLabel = '+ Adicionar',
  itemAriaPrefix = 'Item',
}: EditableListProps) {
  const lastInputRef = useRef<HTMLInputElement | null>(null);

  // Always render at least one row so the empty state is editable.
  const rows = items.length === 0 ? [''] : items;

  function updateAt(i: number, value: string) {
    const next = [...rows];
    next[i] = value;
    onChange(next);
  }

  function removeAt(i: number) {
    const next = rows.filter((_, idx) => idx !== i);
    onChange(next);
  }

  function addRow() {
    onChange([...rows, '']);
    // Focus the new row on the next paint.
    requestAnimationFrame(() => lastInputRef.current?.focus());
  }

  function handleKey(e: KeyboardEvent<HTMLInputElement>, i: number) {
    if (e.key === 'Enter' && i === rows.length - 1) {
      e.preventDefault();
      addRow();
    }
  }

  return (
    <div className="flex flex-col gap-1.5">
      {rows.map((item, i) => (
        <div key={i} className="flex items-center gap-1.5">
          <Input
            value={item}
            placeholder={placeholder}
            aria-label={`${itemAriaPrefix} ${i + 1}`}
            onChange={(e) => updateAt(i, e.target.value)}
            onKeyDown={(e) => handleKey(e, i)}
            ref={i === rows.length - 1 ? lastInputRef : undefined}
          />
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            aria-label={`Remover ${itemAriaPrefix.toLowerCase()} ${i + 1}`}
            onClick={() => removeAt(i)}
            disabled={rows.length === 1 && item === ''}
          >
            <X />
          </Button>
        </div>
      ))}
      <Button
        type="button"
        variant="ghost"
        size="sm"
        onClick={addRow}
        className="self-start text-muted-foreground hover:text-foreground"
      >
        <Plus /> {addLabel}
      </Button>
    </div>
  );
}
