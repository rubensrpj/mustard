# Plano de Waves — Acesso a dados unificado + refresh ao vivo

## Contexto

O sistema deveria abrir o banco SQLite uma vez e reusar a conexão para todas as
consultas de uma mesma operação. Em vez disso, cada chamada reconstrói a conexão
do zero — abre o arquivo, roda o PRAGMA de WAL, executa todo o DDL de criação de
tabelas e percorre a escada de migrações — mesmo quando o esquema já está na
versão atual. Como o `mustard-rt` nasce como um processo novo a cada evento do
Claude Code, esse custo fixo é pago a cada uso de ferramenta; pior, os módulos de
hook reabrem o banco duas ou três vezes dentro da mesma invocação. No leitor, a
listagem de specs ainda abre uma conexão por spec dentro de um laço (problema
N+1) e a tela de workspace varre a tabela inteira de eventos sem filtro. O efeito
observável é a lentidão generalizada que o usuário sente "principalmente nas
querys", tanto na sessão do agente quanto no dashboard.

## Usuários/Stakeholders

Quem opera o Claude Code com Mustard instalado (cada tool use espera os hooks) e
quem usa o dashboard desktop (telas que recarregam dados ao vivo). Pedido feito
pelo Rubens após sentir o projeto "extremamente lento nas querys".

## Métrica de sucesso

Uma invocação de hook abre o arquivo do banco no máximo uma vez e, em regime
estável (esquema na versão atual), não executa DDL nem migrações. A listagem de
specs deixa de escalar o número de aberturas com o número de specs. O dashboard
não reabre conexão a cada render. Builds e testes das três crates passam.

## Não-Objetivos

- **Não** adicionar dependência de pool genérico (`r2d2`/`deadpool`). O dashboard
  é multi-projeto (um arquivo `.db` por projeto); a solução é um cache de conexão
  por caminho dentro do repository, não um pool de conexões para um único banco.
- **Não** criar nova thread de atualização. O dashboard já tem um watcher de
  filesystem (`watcher.rs`) que empurra `dashboard:fs-change` e invalida as
  queries; vamos apoiar nele, não duplicá-lo.
- **Não** preservar caminhos legados com banner/shim. Mustard está em fase de
  desenvolvimento — trocar a API antiga direto, sem camada de compatibilidade.
- **Não** redesenhar o esquema nem reescrever a camada FTS; só somar índices e
  prune onde faltam.

## Critérios de Aceitação

Testáveis, binários (passa/falha). Cada um executável e independente.

- [x] AC-1: Build das três crates passa — Command: `cargo build -p mustard-core -p mustard-rt`
- [x] AC-2: Testes do core passam (inclui novos testes do repository) — Command: `cargo test -p mustard-core`
- [x] AC-3: Build do dashboard (backend Tauri) passa — Command: `cargo build -p mustard-dashboard`
- [x] AC-4: Fast-path de esquema existe — `user_version` é lido como gate de DDL/migração — Command: `bash -c "grep -rq 'user_version' packages/core/src/store && echo ok"`
- [x] AC-5: N+1 eliminado — `list_specs` não chama `spec_view` dentro de laço — Command: `node -e "const fs=require('fs');const s=fs.readFileSync('packages/core/src/reader/sqlite.rs','utf8');const i=s.indexOf('fn list_specs');const body=s.slice(i,i+2500);process.exit(/for\s+\w+\s+in[\s\S]*spec_view\(/.test(body)?1:0)"`

## Tabela de Waves

| Wave | Spec | Role | Depende de | Resumo |
|------|------|------|------------|--------|
| 1 | [[wave-1-library]] | library | — | packages/core: usar o `store::SqliteEventStore` existente como ponto único; schema fast-path via `user_version`, `synchronous=NORMAL`, cache fino por path (`store/db_cache.rs`), queries agregadas (mata N+1), índices faltantes e prune de events |
| 2 | [[wave-2-general]] | general | [[1]] | apps/rt: dispatch constrói 1 repository por invocação e passa aos módulos; tracker/knowledge/economy param de reabrir; replay-como-lookup vira existence query; session_start agrupa queries |
| 3 | [[wave-3-ui]] | ui | [[1]] | apps/dashboard: repository em tauri::State (.manage), with_db reusa conexão; reduzir polling redundante apoiando no FS watcher; reativar paths de knowledge no watcher |

## Critique Coverage

Cada preocupação levantada na conversa, mapeada para wave, não-objetivo ou decisão.

| Item levantado | Categoria | Onde |
|---|---|---|
| "Extremamente lento, principalmente nas querys" | Coberto | Waves 1+2+3 (open-per-call, N+1, full scan) |
| "Use SOLID para criar um único ponto de acesso (repository)" | Coberto | Wave 1 — consolidar no `store::SqliteEventStore` já existente (trait `EventSink` = DIP); sem módulo paralelo |
| "Verifica a atualização automática da tela" | Coberto | Wave 3 — apoiar no FS watcher existente; reativar paths de knowledge; reduzir polling |
| "Talvez um pool ou thread" | Decisão (surface no approve) | Pool descartado a favor de cache-por-path (multi-projeto); thread já existe (watcher). Ver Não-Objetivos |
