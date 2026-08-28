@.claude/scan-map.md

# Dashboard

> Parent: [../../CLAUDE.md](../../CLAUDE.md) | Orchestrator: [../../.claude/mustard/orchestrator.md](../../.claude/mustard/orchestrator.md)

## Guards

<!-- mustard:guards -->
<!-- facts: kind=npm; frameworks=@fontsource-variable/geist, @fontsource/ibm-plex-mono, @fontsource-variable/inter, @fontsource/inter, @tanstack/react-query, react, react-dom, react-router, zustand -->
- `lib/api-client.ts` é o único módulo que fala com o servidor: todo comando sai por `call()` e todo aviso ao vivo entra por `subscribe()`. Ninguém mais monta URL, `fetch` ou `EventSource`.
- Toda chamada `call()` mora só em `src/api/*` ou nos wrappers finos de `src/lib/dashboard.ts`; componentes e `features/` consomem esses wrappers (ou os hooks `useXxx`), nunca chamam `call()` direto.
- Os parâmetros passados ao `call()` vão em camelCase (`repoPath`, `specName`) e o servidor os mapeia para snake_case — não renomeie essas chaves, são o contrato de serialização com o backend.
- Hooks de query seguem o mesmo molde: `queryKey` em array estável com `repoPath`/`spec` como folhas, `enabled: !!repoPath` para não disparar sem projeto, e só então `repoPath as string` na `queryFn`.
- O refresh ao vivo é orientado a evento: o watcher (`lib/watcher.ts`) escuta `dashboard:fs-change` e invalida `queryKey`s por prefixo — ao criar uma página nova, registre a chave lá em vez de fazer polling.
- Os comandos do backend são tolerantes a falha (devolvem vazio quando faltam dados); trate o caso vazio com um empty state em vez de supor que o erro virá pelo `onError`.
<!-- /mustard:guards -->
