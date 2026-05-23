import { ReactNode } from "react";
import { cn } from "@/lib/utils";

export interface PageSurfaceProps {
  children: ReactNode;
  className?: string;
  /**
   * Aplica o ritmo editorial Binance (80px vertical padding via --editorial-band-py).
   * Default true. Use false apenas em sub-páginas embedadas (split-detail content).
   */
  editorial?: boolean;
}

export function PageSurface({ children, className, editorial = true }: PageSurfaceProps) {
  return (
    <div
      className={cn(
        "flex flex-col gap-8 w-full max-w-7xl mx-auto px-6",
        editorial && "py-20",
        className,
      )}
    >
      {children}
    </div>
  );
}
