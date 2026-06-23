import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";

/**
 * Layout de detalhe lateral — comportamento único em todas as páginas.
 *
 * Sem painel: o conteúdo segue na coluna capada/centralizada do AppShell.
 * Com `open`: conteúdo à esquerda, painel à direita, divididos por um arrasto
 * redimensionável. Cada metade tem scroll vertical próprio, então não há
 * scroll horizontal nem barra dupla.
 *
 * Dois modos de container:
 *   - `variant="overlay"` (padrão, legado): um overlay `fixed` cobre a área de
 *     conteúdo (viewport menos a sidebar de 220px e a topbar de 40px —
 *     constantes do grid do AppShell).
 *   - `variant="inline"`: o split ocupa o fluxo normal do pai (sem `fixed`),
 *     para quando o painel mora dentro de um container já posicionado (ex.: a
 *     aba Ondas do detalhe de spec).
 *
 * A largura do painel é fração arrastável, presa entre `MIN_FRACTION` e
 * `MAX_FRACTION` para que nenhuma metade desapareça.
 */

const MIN_FRACTION = 0.25;
const MAX_FRACTION = 0.75;
const DEFAULT_FRACTION = 0.5;

export function SplitDetail({
  open,
  panel,
  children,
  variant = "overlay",
  initialFraction = DEFAULT_FRACTION,
}: {
  open: boolean;
  panel: ReactNode;
  children: ReactNode;
  /** Container mode. `overlay` is the legacy fixed full-area split; `inline`
   *  flows inside the parent so the split can be embedded in a panel. */
  variant?: "overlay" | "inline";
  /** Starting panel width as a fraction of the container (0.25–0.75). */
  initialFraction?: number;
}) {
  // Panel width as a fraction of the container. The drag handle mutates this.
  const [panelFraction, setPanelFraction] = useState<number>(() =>
    clampFraction(initialFraction),
  );
  const containerRef = useRef<HTMLDivElement | null>(null);
  const draggingRef = useRef(false);

  const onPointerMove = useCallback((e: PointerEvent) => {
    if (!draggingRef.current) return;
    const el = containerRef.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    if (rect.width === 0) return;
    // Pointer x relative to the container left edge → content width fraction;
    // the panel takes the complement.
    const contentFraction = (e.clientX - rect.left) / rect.width;
    setPanelFraction(clampFraction(1 - contentFraction));
  }, []);

  const stopDragging = useCallback(() => {
    draggingRef.current = false;
    document.body.style.removeProperty("cursor");
    document.body.style.removeProperty("user-select");
  }, []);

  // Bind the move/up listeners on window so the drag survives the pointer
  // leaving the thin divider. Cleaned up on unmount.
  useEffect(() => {
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", stopDragging);
    return () => {
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", stopDragging);
    };
  }, [onPointerMove, stopDragging]);

  if (!open) return <>{children}</>;

  const startDragging = () => {
    draggingRef.current = true;
    // Lock the cursor + disable text selection during the drag for a clean feel.
    document.body.style.setProperty("cursor", "col-resize");
    document.body.style.setProperty("user-select", "none");
  };

  const containerClass =
    variant === "overlay"
      ? "fixed left-[220px] top-[40px] right-0 bottom-0 z-30 flex bg-background"
      : // Inline: flow inside the parent. `items-stretch` (flex default) makes
        // both columns share the tallest height, so the panel's `h-full` has a
        // concrete height to fill even when the parent is unsized.
        "flex w-full min-h-0";

  // The overlay variant owns the content padding (it is the page surface);
  // the inline variant flows inside an already-padded parent, so it only
  // reserves a small gutter before the divider.
  const contentClass =
    variant === "overlay"
      ? "min-w-0 overflow-y-auto px-6 py-6"
      : "min-w-0 overflow-y-auto pr-3";

  return (
    <div ref={containerRef} className={containerClass}>
      <div
        className={contentClass}
        style={{ width: `${(1 - panelFraction) * 100}%` }}
      >
        {children}
      </div>
      {/* Drag handle — a thin column the user grabs to resize. Keyboard users
          can nudge the divider with the arrow keys. */}
      <div
        role="separator"
        aria-orientation="vertical"
        aria-label="Redimensionar painel"
        tabIndex={0}
        onPointerDown={startDragging}
        onKeyDown={(e) => {
          if (e.key === "ArrowLeft") {
            e.preventDefault();
            setPanelFraction((f) => clampFraction(f + 0.05));
          } else if (e.key === "ArrowRight") {
            e.preventDefault();
            setPanelFraction((f) => clampFraction(f - 0.05));
          }
        }}
        className="w-1.5 shrink-0 cursor-col-resize bg-border hover:bg-[--primary]/50 focus-visible:bg-[--primary] focus-visible:outline-none transition-colors"
      />
      <aside
        className={
          variant === "overlay"
            ? "min-w-0 flex flex-col overflow-hidden border-l border-border"
            : "min-w-0 flex flex-col overflow-hidden pl-3"
        }
        style={{ width: `${panelFraction * 100}%` }}
      >
        {panel}
      </aside>
    </div>
  );
}

/** Clamp a width fraction into the [MIN_FRACTION, MAX_FRACTION] band. */
function clampFraction(f: number): number {
  if (!Number.isFinite(f)) return DEFAULT_FRACTION;
  return Math.min(MAX_FRACTION, Math.max(MIN_FRACTION, f));
}
