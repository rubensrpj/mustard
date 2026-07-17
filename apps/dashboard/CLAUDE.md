@.claude/scan-map.md

# Dashboard

> Parent: [../../CLAUDE.md](../../CLAUDE.md) | Orchestrator: [../../.claude/CLAUDE.md](../../.claude/CLAUDE.md)



## Guards

<!-- mustard:guards -->
<!-- facts: kind=npm; frameworks=@fontsource-variable/geist, @fontsource/ibm-plex-mono, @fontsource-variable/inter, @fontsource/inter, @tanstack/react-query, @tauri-apps/api, @tauri-apps/plugin-dialog, @tauri-apps/plugin-log, @tauri-apps/plugin-opener, @tauri-apps/plugin-store, @tauri-apps/plugin-updater, @tauri-apps/plugin-window-state -->
- Toda chamada `invoke()` mora só em `src/api/*` ou nos wrappers finos de `src/lib/dashboard.ts`; componentes e `features/` consomem esses wrappers (ou os hooks `useXxx`), nunca chamam `invoke()` direto.
- Os parâmetros passados ao `invoke()` vão em camelCase (`repoPath`, `specName`) e o serde do Rust os mapeia para snake_case — não renomeie essas chaves, são o contrato de serialização com o backend.
- Hooks de query seguem o mesmo molde: `queryKey` em array estável com `repoPath`/`spec` como folhas, `enabled: !!repoPath` para não disparar sem projeto, e só então `repoPath as string` na `queryFn`.
- O refresh ao vivo é orientado a evento: o watcher (`lib/watcher.ts`) escuta `dashboard:fs-change` e invalida `queryKey`s por prefixo — ao criar uma página nova, registre a chave lá em vez de fazer polling.
- Os comandos Tauri são tolerantes a falha (devolvem vazio quando faltam dados); trate o caso vazio com um empty state em vez de supor que o erro virá pelo `onError`.
<!-- /mustard:guards -->
