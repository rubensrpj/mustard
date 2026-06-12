import { ReactNode } from "react";
import { cn } from "@/lib/utils";

export interface PageSurfaceProps {
  children: ReactNode;
  className?: string;
  /**
   * Aplica o ritmo editorial: respiro vertical confortável (topo enxuto para
   * não abrir um vão grande sob o topbar, base mais folgada).
   * Default true. Use false apenas em sub-páginas embedadas (split-detail content).
   */
  editorial?: boolean;
}

export function PageSurface({ children, className, editorial = true }: PageSurfaceProps) {
  return (
    <div
      className={cn(
        "flex flex-col gap-8 w-full px-6",
        editorial && "pt-8 pb-16",
        className,
      )}
    >
      {children}
    </div>
  );
}
