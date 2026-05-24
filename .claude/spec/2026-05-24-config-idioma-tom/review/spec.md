# Onda de Revisão — Configuração de idioma e tom

## Resumo

Revisão final agregando o trabalho das três ondas. Lê o diff completo desde o início da spec, valida cada Critério de Aceitação, e revisa qualidade do código com olhar de sênior.

## Tarefas

### Review Agent

- [ ] Conferir que cada AC do `wave-plan.md` (AC-1 até AC-10) está coberto por código real — não só comentário ou TODO.
- [ ] Conferir que `lang` e `tone` defaults estão batendo com o template do `mustard.json` (`pt-BR`, `didactic`).
- [ ] Conferir que a migração de `specLang` é idempotente: rodar duas vezes seguidas não duplica nada nem corrompe o arquivo.
- [ ] Conferir que os banners do `mustard-rt` em `tone: technical` mostram texto idêntico ao que era mostrado antes desta spec (sem regressão visual em projetos que não trocam o tom).
- [ ] Conferir que `Preferences.tsx` não foi contaminada com lógica de `mustard.json`. Continua mexendo só no estado zustand do dashboard.
- [ ] Conferir que `tone: caveman` realmente comprime — não basta tirar ponto-final. Conferir 3 banners de hooks diferentes (`bash_guard`, `path_guard`, `close_gate`) em caveman e ver se a redução chega perto de 50-70%.
- [ ] **Conferir que `tone` NÃO afeta estruturas parseáveis.** Gerar uma spec com `tone: caveman` ativo, rodar `pipeline_state_ingest` nela (via `meta.json` da spec B), confirmar que `phase`/`stage`/`outcome` saem literais. Conferir que `## Contexto`/`## Plano` e headings de seção do `.md` continuam reconhecíveis pelo parser.
- [ ] **Conferir que slug de spec respeita `lang`.** Gerar spec nova num projeto com `lang: pt-BR` (título PT, ex.: "Adicionar email no usuário") e confirmar slug em PT com acentos normalizados (`adicionar-email-no-usuario`). Repetir com `lang: en-US` (título EN).
- [ ] **Conferir que CLI inteiro foi coberto.** Grep por `println!\|eprintln!` em `apps/cli/src/commands/**/*.rs` — toda ocorrência user-facing passa por `tr!`. Stack errors internos podem continuar crus.
- [ ] Conferir que código-fonte, comentários, identificadores e paths ficaram em inglês.
- [ ] Conferir que a inconsistência `W3` vs `onda 3` no dashboard foi eliminada — grep por `[W]\d` literal em todos os componentes do workspace deve retornar zero.
- [ ] Build (`cargo build --workspace`), lint (`cargo clippy --workspace`, `pnpm --filter mustard-dashboard lint`) e type-check (`pnpm --filter mustard-dashboard build`) passam.

## Limites

Não escreve código novo. Lê o que as Ondas 1-3 produziram e aponta o que precisa voltar para correção. Em caso de reprovação, devolve para a onda específica — não conserta direto.
