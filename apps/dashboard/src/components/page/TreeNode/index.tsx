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
  const indent = depth === 0 ? "" : "ml-3 pl-3 border-l border-dashed border-[--ds-surface-hover]";

  const row = (
    <div
      onClick={(e) => {
        e.stopPropagation();
        onSelect?.(node);
      }}
      className={cn(
        "flex items-center gap-1.5 py-1 px-1.5 rounded-[--ds-radius-sm] cursor-pointer",
        "hover:bg-[--ds-surface-hover]",
        "text-[13px] text-[--ds-text-primary]",
      )}
    >
      {hasChildren ? (
        <ChevronRight
          size={12}
          className="text-[--ds-text-tertiary] transition-transform group-open:rotate-90"
        />
      ) : (
        <span className="inline-block w-3" />
      )}
      <span className="truncate flex-1">{node.label}</span>
      {node.meta ? (
        <span className="text-[11px] text-[--ds-text-tertiary] font-mono">{node.meta}</span>
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
