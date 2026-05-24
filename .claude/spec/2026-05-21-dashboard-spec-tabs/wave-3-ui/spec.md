# Wave 3 — Trace: painel expandido com payload + ts + actor

## Resumo

A aba "Trace" usa `<ExecutionTrace>` (hierárquico spec → wave → agent → tool). O expand já existe mecanicamente, mas o painel expandido não evidencia `payload + timestamp + actor`. O usuário quer cada evento clicável abrindo um painel inline com essas três coisas visíveis sem ginástica. Wave 3 ajusta `<ToolEventRow>` (e qualquer outro `*Row` similar) pra renderizar essa tríade de forma consistente.

## Contexto

`apps/dashboard/src/components/trace/ExecutionTrace.tsx` constrói uma árvore de nós (`spec/wave/agent/tool`). `ToolEventRow.tsx` é o leaf — o grep mostra que ele já tem `aria-expanded={showRaw}` (linha 219). O conteúdo do "expand" hoje provavelmente é só um dump JSON cru — não há recorte estruturado com label/ts/actor. Wave 3 transforma o conteúdo expandido em três blocos:

1. **Header do painel:** `actor.kind` (Agent/Tool/Hook) + `actor.id` (quando presente) + `ts` formatado (`relativeTime` ou absolute em hover).
2. **Resumo curto:** uma linha-síntese do que o evento fez (já calculada upstream — `payload_summary` ou `label`).
3. **Payload completo:** bloco `<pre>` com JSON pretty-printed, com `Copy` button (opcional, low-cost: `navigator.clipboard.writeText`).

A mesma estrutura se aplica aos demais `Row*` se houver (Wave inspeciona, replica).

## Arquivos

```
apps/dashboard/src/components/trace/ExecutionTrace.tsx       — abrir painel ao clicar no row, não só no chevron
apps/dashboard/src/components/trace/ToolEventRow.tsx         — painel com header (actor+ts) + summary + payload
apps/dashboard/src/components/trace/*.tsx                    — replicar tratamento se outros Row* existirem
```

## Tarefas

- [ ] Ler `ExecutionTrace.tsx` para confirmar: clique no row inteiro alterna `open`, não só o chevron. Se hoje só o chevron alterna, adicionar `onClick` no row level (com `e.stopPropagation` no chevron pra não duplicar).
- [ ] Em `ToolEventRow.tsx`: refatorar o conteúdo expandido para a estrutura:
  ```tsx
  {open && (
    <div className="ml-6 mt-1 rounded border border-border bg-card/40 p-2 text-[12px]">
      <div className="flex items-center gap-2 text-muted-foreground text-[11px]">
        <span>{actor.kind}{actor.id ? `:${actor.id}` : ""}</span>
        <span>·</span>
        <time dateTime={ts} title={ts}>{relativeTime(ts)}</time>
      </div>
      {summary && <p className="mt-1 text-foreground/80">{summary}</p>}
      <pre className="mt-2 max-h-72 overflow-auto rounded bg-muted/30 p-2 font-mono text-[11px] leading-tight">
        {JSON.stringify(payload, null, 2)}
      </pre>
    </div>
  )}
  ```
- [ ] Listar outros componentes `apps/dashboard/src/components/trace/*Row*.tsx` (se `PhaseEventRow`, `WaveEventRow`, etc. existirem). Aplicar o mesmo padrão. Skip se não existirem.
- [ ] Garantir que `ts`, `actor`, `payload` chegam até o leaf. Se a árvore de tipos no `useExecutionTrace` (ou equivalente) ainda não inclui esses campos, propagar pela árvore (não inventar — apenas surfacear os que já vêm do Tauri command).
- [ ] Adicionar botão `Copy` opcional no `<pre>` (chip pequeno, `navigator.clipboard.writeText(JSON.stringify(payload))`). Aria-label: "Copiar payload".
- [ ] Build: `pnpm --filter mustard-dashboard build`
- [ ] Type-check faz parte do build.

## Acceptance Criteria

- [ ] AC-W3-1: Dashboard tipa e builda — Command: `pnpm --filter mustard-dashboard build`
- [ ] AC-W3-2: `ToolEventRow.tsx` referencia `actor`, `ts` e `payload` no bloco expandido — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/trace/ToolEventRow.tsx','utf8');const ok=/actor/.test(s)&&/ts|timestamp/.test(s)&&/payload/.test(s);process.exit(ok?0:1)"`
- [ ] AC-W3-3: Clique no row alterna expand (não só o chevron) — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/trace/ExecutionTrace.tsx','utf8');const ok=/onClick/.test(s)&&/setOpen|toggle/.test(s);process.exit(ok?0:1)"`

## Limites

- `apps/dashboard/src/components/trace/ExecutionTrace.tsx`
- `apps/dashboard/src/components/trace/ToolEventRow.tsx`
- Outros `apps/dashboard/src/components/trace/*Row*.tsx` (se existirem)

## Network

- Parent: [[2026-05-21-dashboard-spec-tabs]]
- Depende: [[wave-1-ui]] (precisa do `<SpecDetailDashboard>` montando a aba Trace)
