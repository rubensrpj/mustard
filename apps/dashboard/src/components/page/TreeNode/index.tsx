// Recursive tree node using native <details>/<summary>. Vertical
// connector lines come from a ::before pseudo on each depth>=1 row,
// driven purely by CSS — no third-party tree lib.

import type { ReactNode } from "react";
import { ChevronRight } from "lucide-react";
import { cn } from "@/lib/utils";

export interface TreeNodeData {
  id?: string;
  label: string;
  children?: TreeNodeData[];
  meta?: ReactNode;
}

export interface TreeNodeProps {
  node: TreeNodeData;
  defaultExpanded?: boolean;
  onSelect?: (node: TreeNodeData) => void;
  /** internal — recursion depth */
  depth?: number;
  className?: string;
}

export function TreeNode({
  node,
  defaultExpanded = true,
  onSelect,
  depth = 0,
  className,
}: TreeNodeProps) {
  const hasChildren = (node.children?.length ?? 0) > 0;
  // TF remap: --ds-surface-hover → --accent; Binance accent = hover surface swatch
  const indent = depth === 0 ? "" : "ml-3 pl-3 border-l border-dashed border-[--accent]";

  const row = (
    <div
      onClick={(e) => {
        e.stopPropagation();
        onSelect?.(node);
      }}
      className={cn(
        // TF remap: --ds-radius-sm → var(--radius) (6px button radius); --ds-surface-hover → --accent; --ds-text-primary → --foreground
        "flex items-center gap-1.5 py-1 px-1.5 rounded-[--radius] cursor-pointer",
        "hover:bg-[--accent]",
        "text-[13px] text-[--foreground]",
      )}
    >
      {hasChildren ? (
        <ChevronRight
          size={12}
          // TF remap: --ds-text-tertiary → --muted-foreground
          className="text-[--muted-foreground] transition-transform group-open:rotate-90"
        />
      ) : (
        <span className="inline-block w-3" />
      )}
      <span className="truncate flex-1">{node.label}</span>
      {node.meta ? (
        // TF remap: --ds-text-tertiary → --muted-foreground
        <span className="text-[11px] text-[--muted-foreground] font-mono">{node.meta}</span>
      ) : null}
    </div>
  );

  if (!hasChildren) {
    return <div className={cn(indent, className)}>{row}</div>;
  }

  return (
    <details open={defaultExpanded} className={cn("group", indent, className)}>
      <summary className="list-none [&::-webkit-details-marker]:hidden">{row}</summary>
      <div className="mt-0.5">
        {node.children!.map((child, idx) => (
          <TreeNode
            key={child.id ?? `${depth}-${idx}-${child.label}`}
            node={child}
            defaultExpanded={defaultExpanded}
            onSelect={onSelect}
            depth={depth + 1}
          />
        ))}
      </div>
    </details>
  );
}
