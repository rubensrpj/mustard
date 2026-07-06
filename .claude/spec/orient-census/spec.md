---
id: spec.orient-census
---

# census de orientação no início de cada pedido — o terreno já mapeado entra na janela, a IA não sai grepando

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Critérios de Aceitação, inject = enxerto de contexto na janela do agente) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

O `/scan` já mineira o repositório inteiro em `.claude/grain.model.json`: os subprojetos, o tipo de cada um, e — o mais valioso — os arquivos-exemplar por papel (`role`) com `start_line`. Esse é o "onde as coisas moram", já pronto em disco.

O problema: esse mapa **é consumido mas descartado**. O roteador lê o grain no `scope-classify`, mas o `run_classify` destila tudo para três contadores (`fileCount` / `layerCount` / `newEntityCount`) e devolve só isso — a IA nunca recebe o mapa de volta. E o contexto ambiente (o `CLAUDE.md` raiz + o rodapé `## Guards` de cada subprojeto) diz *"Pesquise via `mustard-rt run feature` para localizar"* — ou seja, **manda a IA ir procurar** em vez de já entregar o terreno. Resultado: no início de cada análise a IA faz cold-start com `grep`/`glob`, redescobrindo o que o `/scan` já pagou para mapear.

Esta correção fecha o buraco com um **census de orientação** determinístico, em dois níveis, cada um no hook certo — sem abrir pipeline nem quebrar a economia de roteamento (o mapa é entregue como moldura ambiente, não como consulta que a IA precisa disparar).

**Nível 1 — Terreno** (no `session_start_inject`, 1× por sessão): uma linha por subprojeto (`nome · tipo · Nf — papel`), projetada do grain. Estável → paga-se uma vez.

**Nível 2 — Pontos de entrada** (no `prompt_submit_inject`, por pedido, com trava): rankeia o **inventário de módulos do repo inteiro** (`grain.modules[]`) pelos tokens de **conceito** do pedido que aparecem no **caminho** do arquivo, pesado por **raridade no corpus (`N/df`, IDF)** — token raro (`inject`, `close_gate`) vence os comuns (`src`, ou um nome de subprojeto). Os nomes de subprojeto são **subtraídos** do pedido (dizem ONDE, não O QUÊ); arquivos de **teste** são excluídos (`is_test_path`). Os melhores casos são agrupados por subprojeto (`path:line — símbolo`), teto ≤2 subprojetos × ≤4 arquivos. **Sem fallback**: se nenhum conceito casa (ex.: lacuna PT→EN "telemetria"→"telemetry"), o Nível 2 fica calado — o Terreno + o digest cobrem, sem despejo estrutural fingindo relevância. **Nunca** injeta snippet (o read-view do grain nem desserializa corpo). Travas: pula em `/mustard:*`, pula quando já há spec ativa, fail-open.

O código vive numa função core única `compute_orientation(root, intent)`, exposta como `mustard-rt run orient` **e** consumida pelos dois injects (core direto, sem facade). O template do `CLAUDE.md` (raiz + rodapé de subprojeto + `refs/locating-code.md`) troca *"vá pesquisar para se orientar"* por *"o terreno já está na sua janela; leia os pontos de entrada, use o digest só para conceito além do census"*.

Âncoras (subprojeto `apps/rt`, mais o template em `apps/cli`):
- `apps/rt/src/commands/mod.rs` — enum `RunCmd` (nova variante `Orient`) + braço no `dispatch()` (a Guard do rt exige os DOIS registros).
- `apps/rt/src/commands/orient.rs` (novo) — o comando `run orient`; e a função core `compute_orientation` (em `shared/` ou no módulo, consumida também pelos injects — sem wrapper).
- `apps/rt/src/hooks/session/session_start_inject.rs` — injeta o Nível 1 (Terreno).
- `apps/rt/src/hooks/session/prompt_submit_inject.rs` — injeta o Nível 2 (Pontos de entrada), com as travas; reusa a checagem `current_spec` que o arquivo já faz.
- `.claude/grain.model.json` — fonte: `projects[]`+`skeleton[]` (Terreno) e `modules[]` com `declarations[].line` (Nível 2, ranking por conceito).
- `apps/cli/templates/CLAUDE.md`, `apps/cli/templates/refs/locating-code.md`, e o rodapé `## Guards` do template de subprojeto — trocar "vá pesquisar" por "terreno já na janela".

## Usuários/Stakeholders

- **IA orquestradora / subagentes** — abrem a sessão e cada pedido já sabendo o terreno e os pontos de entrada; param de grepar para se localizar.
- **Usuário do CLI** — análise que começa do mapa é mais rápida e barata (menos round-trips de busca).

## Métrica de sucesso

- No início de uma análise, a IA cita arquivos-alvo a partir do census (não a partir de um grep de descoberta).
- O census cabe em ~6 linhas (Terreno) + ≤8 linhas (Pontos de entrada) — negligível vs. os round-trips de grep que evita.
- Determinístico e byte-estável (sem IA, sem timestamps/caminhos voláteis).

## Não-Objetivos

- **Não** abre pipeline nem cria wave/PLAN — é enxerto ambiente.
- **Não** substitui o digest (`run feature`): o census orienta; o digest segue para localização por conceito além do que o census cobre.
- **Não** injeta corpo de código (snippet) nem inventário completo de arquivos.
- **Não** dispara em `/mustard:*` nem quando há spec ativa.

## Critérios de Aceitação

- **AC-1** — `run orient` sem intent lista 1 linha por subprojeto (nome + tipo + contagem de arquivos), byte-estável.
  Command: `mustard-rt run orient`
- **AC-2** — `run orient --intent` adiciona pontos de entrada relevantes ao conceito (`path:line`), rankeados por raridade e agrupados por subprojeto (≤2×≤4), sem snippet — ex.: "hook de inject" → arquivos `*_inject.rs`; "close gate pipeline" → `close_gate.rs`/`close_pipeline.rs`.
  Command: `mustard-rt run orient --intent "adicionar hook de inject no rt"`
- **AC-3** — grain ausente → `run orient` sai 0 com saída vazia (fail-open, paridade com os injects).
  Command: `cargo test orient_fail_open`
- **AC-4** — build + testes verdes, incluindo o snapshot novo do `run orient`.
  Command: `cargo test`
- **AC-5** — o template do orquestrador deixou de mandar "vá pesquisar para se orientar" (terreno agora é entregue).
  Command: `cargo build`

<!-- PLAN -->

## Arquivos

- `apps/rt/src/commands/orient.rs` (novo) + `compute_orientation` core.
- `apps/rt/src/commands/mod.rs` — variante `Orient` no enum + braço no `dispatch()`.
- `apps/rt/src/hooks/session/session_start_inject.rs` — Nível 1 (Terreno).
- `apps/rt/src/hooks/session/prompt_submit_inject.rs` — Nível 2 (Pontos de entrada) + travas.
- `apps/cli/templates/CLAUDE.md`, `apps/cli/templates/refs/locating-code.md`, rodapé `## Guards` do template de subprojeto — trocar "vá pesquisar" por "terreno já na janela".
- Snapshot `insta` do `run orient` (determinístico).

## Limites

IN: função core `compute_orientation` + `run orient` (bare); Nível 1 no session_start_inject; Nível 2 gated no prompt_submit_inject; edição do template para entregar o terreno; snapshot determinístico.
OUT: pipeline/wave; mudança no digest; dashboard; qualquer injeção de corpo de código.