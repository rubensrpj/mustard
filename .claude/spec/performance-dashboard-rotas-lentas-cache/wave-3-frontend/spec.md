# wave-3-frontend

## Resumo

Front-end consome o push granular e abandona a invalidação em massa das 13 chaves de consulta

## Rede

- Pai: [[performance-dashboard-rotas-lentas-cache]]
- Depende de: [[wave-2-tauri]]

## Tarefas

- [ ] Em watcher.ts (linhas 14-56), substituir a invalidação em massa: aplicar o snapshot recebido via queryClient.setQueryData nas chaves de specs e invalidar pontualmente apenas as chaves do kind afetado
- [ ] Adicionar em dashboard.ts o binding tipado do evento dashboard:specs-snapshot (listen + tipo do payload), seguindo o padrão dos bindings existentes
- [ ] Alinhar as invalidações de useSpecActions.ts ao novo fluxo (pontuais, por spec, sem chaves globais)
- [ ] Garantir que a rota de detalhe de spec não refaz as 5 consultas quando chega um push — os dados entram por setQueryData

## Arquivos

- `apps/dashboard/src/lib/watcher.ts`
- `apps/dashboard/src/lib/dashboard.ts`
- `apps/dashboard/src/hooks/useSpecActions.ts`
