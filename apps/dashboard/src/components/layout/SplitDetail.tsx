import type { ReactNode } from "react";

/**
 * Layout de detalhe lateral — comportamento único em todas as páginas.
 *
 * Sem painel: o conteúdo segue na coluna capada/centralizada do AppShell.
 * Com `open`: um overlay `fixed` cobre exatamente a área de conteúdo
 * (viewport menos a sidebar de 220px e a topbar de 48px — constantes do grid
 * do AppShell), dividido 50/50 — conteúdo à esquerda, painel à direita. Cada
 * metade tem scroll vertical próprio, então não há scroll horizontal nem
 * barra dupla.
 */
export function SplitDetail({
  open,
  panel,
  children,
}: {
  open: boolean;
  panel: ReactNode;
  children: ReactNode;
}) {
  if (!open) return <>{children}</>;
  return (
    <div className="fixed left-[220px] top-[48px] right-0 bottom-0 z-30 flex bg-background">
      <div className="w-1/2 min-w-0 overflow-y-auto px-6 py-6">{children}</div>
      <aside className="w-1/2 border-l border-border flex flex-col overflow-hidden">
        {panel}
      </aside>
    </div>
  );
}
