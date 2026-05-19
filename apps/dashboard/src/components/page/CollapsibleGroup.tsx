import { useState, type ReactNode } from "react";
import { ChevronRight, ChevronDown } from "lucide-react";

/**
 * Collapsed-by-default disclosure group for high-cardinality content
 * (132 closed specs, 200+ archived events, etc.). Shows a hint when
 * collapsed so the user knows what's hidden without expanding.
 */
export interface CollapsibleGroupProps {
  /** Group label (rendered uppercase + tracked). */
  label: string;
  /** Count shown next to label. When undefined, renders no parenthetical. */
  count?: number;
  /** Hint shown when collapsed — describes what's inside. */
  hint?: ReactNode;
  /** Content rendered when expanded. */
  children: ReactNode;
  /** Force initial state (default: closed). */
  defaultOpen?: boolean;
  className?: string;
}

export function CollapsibleGroup({
  label,
  count,
  hint,
  children,
  defaultOpen = false,
  className,
}: CollapsibleGroupProps) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <div className={`flex flex-col gap-1 ${className ?? ""}`}>
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        className="flex items-center gap-2 text-left mt-2 group w-fit"
      >
        {open ? (
          <ChevronDown className="h-3.5 w-3.5 text-muted-foreground group-hover:text-foreground transition-colors" />
        ) : (
          <ChevronRight className="h-3.5 w-3.5 text-muted-foreground group-hover:text-foreground transition-colors" />
        )}
        <h3 className="text-[11px] uppercase tracking-wider font-medium text-muted-foreground/70 group-hover:text-muted-foreground transition-colors">
          {label}
          {count !== undefined && ` (${count})`}
        </h3>
        <span className="text-[11px] text-muted-foreground/50 font-normal normal-case">
          {open ? "ocultar" : "mostrar"}
        </span>
      </button>
      {!open && hint && (
        <p className="text-[11px] text-muted-foreground/60 leading-snug pl-5">{hint}</p>
      )}
      {open && children}
    </div>
  );
}
