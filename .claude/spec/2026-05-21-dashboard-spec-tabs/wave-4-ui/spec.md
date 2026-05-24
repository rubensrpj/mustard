# Wave 4 — Qualidade: AC details + link de teste

## Resumo

Hoje a aba "Qualidade" mostra um veredito (passou/falhou) sem revelar o contexto de cada AC. O usuário quer ver cada AC com `id`, `label`, `command`, `status`, e `last_run_at`. Quando o comando referencia um arquivo de teste (caso comum: `cargo test --lib path::module` ou `node -e "..."`), surface o link pra abrir o arquivo. A query Tauri já devolve esses campos no `SpecQualityItem` — Wave 4 é só repaginar a UI.

## Contexto

`SpecQualityTab.tsx` consome `items: SpecQualityItem[]` com a forma já correta (em `apps/dashboard/src/lib/types/specs.ts`):

```ts
interface SpecQualityItem {
  ac_id: string;
  ac_label: string | null;
  status: string;       // pass | fail | skip | unknown
  wave: number | null;
  command: string | null;
  last_run_at: string | null;
  fail_reason: string | null;
}
```

O componente atual provavelmente só mostra `ac_id` + status pill. Wave 4 expande pra layout:

```
[status pill] AC-1 — {label}
            cmd: `{command}`
            última execução: {relativeTime(last_run_at)}
            [link arquivo] (quando heurística reconhecer um path no command)
            [fail_reason em vermelho, quando status=fail]
```

O link de arquivo usa heurística simples: se `command` contém `\bpath\b(\.module)?` ou um path explícito (`./apps/...`, `packages/...`), abre `vscode://file/<absolute>` ou exibe o trecho clicável. Pra `cargo test` específico (`-p crate --lib mod::sub`), surface `path do mod` como `<crate>/src/<mod>.rs` (best-effort, fallback silencioso quando não resolver).

## Arquivos

```
apps/dashboard/src/components/specs/SpecQualityTab.tsx       — repaginar layout
apps/dashboard/src/lib/quality-link.ts                       — NOVO util: extrair link de teste do command
```

## Tarefas

- [ ] Criar `apps/dashboard/src/lib/quality-link.ts` com `export function extractTestLink(command: string | null): string | null`. Heurísticas:
  - `cargo test -p <crate> --lib <mod>::<sub>` → `<crate-detected-by-workspace-cargo>/src/<mod>/<sub>.rs` ou `<crate>/src/<mod>.rs`. Fallback: nenhum link (retorna null).
  - `node -e "require('./apps/...')"`, `require('packages/...')`, ou path literal entre aspas → extrai o path.
  - `pnpm --filter <pkg> test <file>` → resolve para o `<file>`.
  - Linha que contém `.rs`, `.ts`, `.tsx`, `.js` standalone → link direto.
  Sem rocket science — sucesso parcial é OK; sem link → retorna `null` e a UI não renderiza nada.
- [ ] Testar `extractTestLink` com casos comuns. Suite de unit-tests inline (vitest se houver; senão, deixar TODO).
- [ ] Em `SpecQualityTab.tsx`: reescrever o render dos items.
  - Container: `<ul>` com `<li>` por AC.
  - Cabeçalho do `<li>`: `<StatusPill status={item.status} />` + `<code>{item.ac_id}</code>` + `<span>{item.ac_label}</span>` (ou `{item.ac_id}` se label vazio).
  - Linha do comando: `<div className="font-mono text-[11px] text-muted-foreground"><span>cmd:</span> <code>{item.command}</code></div>` — usar `whitespace-pre-wrap break-all` pra comandos longos.
  - Linha tempo: `<time>última execução {relativeTime(item.last_run_at)}</time>` quando presente.
  - Link de teste: `const link = extractTestLink(item.command); link && <a href={link} className="text-[--color-accent-mustard] hover:underline">abrir arquivo</a>`. Em ambiente Tauri, abrir via `import { open } from '@tauri-apps/plugin-shell'` se já configurado; senão, fallback `<a target="_blank">`.
  - Fail reason: quando `status === 'fail'`, renderizar `<pre className="text-[--color-error] text-[11px] whitespace-pre-wrap">{item.fail_reason}</pre>`.
- [ ] Skeleton para loading e empty state — preservar o que já tem.
- [ ] Build: `pnpm --filter mustard-dashboard build`

## Acceptance Criteria

- [ ] AC-W4-1: Dashboard tipa e builda — Command: `pnpm --filter mustard-dashboard build`
- [ ] AC-W4-2: `SpecQualityTab.tsx` referencia `ac_id`, `command`, `last_run_at`, `fail_reason` — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecQualityTab.tsx','utf8');const need=['ac_id','command','last_run_at','fail_reason'];process.exit(need.every(k=>s.includes(k))?0:1)"`
- [ ] AC-W4-3: `extractTestLink` existe e é importado em `SpecQualityTab.tsx` — Command: `node -e "const fs=require('fs');const u=fs.existsSync('apps/dashboard/src/lib/quality-link.ts');const s=fs.readFileSync('apps/dashboard/src/components/specs/SpecQualityTab.tsx','utf8');process.exit(u&&/quality-link|extractTestLink/.test(s)?0:1)"`

## Limites

- `apps/dashboard/src/components/specs/SpecQualityTab.tsx`
- `apps/dashboard/src/lib/quality-link.ts` (novo)

## Network

- Parent: [[2026-05-21-dashboard-spec-tabs]]
- Depende: [[wave-1-ui]]
