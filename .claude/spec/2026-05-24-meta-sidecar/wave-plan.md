# Metadados de spec em arquivo lateral (meta.json)

## PRD

## Contexto

Hoje, cada spec do Mustard guarda seus metadados — fase atual, status, escopo, idioma, data de checkpoint, parent — como linhas no início do próprio `spec.md`, no formato `### Stage: Plan`, `### Outcome: Active`, e assim por diante. Toda vez que o Mustard precisa saber em que pé uma spec está, ele abre o arquivo, varre as linhas, faz match de regex e normaliza o valor. O dashboard faz o mesmo. O parser que faz isso (`apps/rt/src/run/spec_sections.rs`) precisa reconhecer cada heading em duas variantes (`## Context` e `## Contexto`, `## Files` e `## Arquivos`, etc.), o que cria uma tabela de variantes frágil e difícil de manter.

Tudo isso pra ler 7 ou 8 campos que poderiam viver num JSON simples.

Esta spec resolve isso de uma vez. Cada pasta `.claude/spec/{nome}/` ganha um `meta.json` lateral ao `spec.md`. Esse JSON guarda os metadados; o `.md` continua sendo o que o humano lê (narrativa, ACs, tarefas) — limpo, sem headers de máquina. O parser do `mustard-rt` lê o JSON em vez de varrer linhas.

Como efeito colateral, a tabela de variantes some, e a próxima spec de idioma/tom não corre risco de quebrar o parser.

## Usuários/Stakeholders

- O próprio Mustard (`pipeline_state_ingest`, dashboard, hooks) — passa a ler estado sem regex.
- Quem mantém o código (deixa de gerenciar tabela de variantes PT/EN).
- Quem vai mexer na próxima spec de idioma e tom (precisa que metadados não dependam de tradução).

## Métrica de sucesso

Quando o `pipeline_state_ingest` precisa saber a fase atual de uma spec, ele abre um JSON e lê o campo. Sem varrer linhas, sem regex, sem tabela de variantes. Mesmo dashboard.

## Não-Objetivos

- Não migrar conteúdo narrativo (PRD, Plano, Tarefas, ACs) pra JSON. Isso continua sendo markdown legível por humano.
- Não mexer no SQLite. A fonte autoritativa do estado vivo continua sendo a tabela de eventos (`pipeline.status`, `pipeline.phase`).
- Não criar interface de edição visual do `meta.json` no dashboard. A spec C (idioma+tom) cuida da parte de UI.
- Não introduzir versionamento do schema. Versão 1, fim. Se um dia mudar, vira nova spec.
- Não traduzir nenhum valor: `stage`, `outcome`, `phase`, `scope` ficam sempre em inglês no JSON. Idioma livre só no campo `lang`.

## Critérios de Aceitação

- [ ] **AC-1.** Toda spec sob `.claude/spec/**` tem um `meta.json` válido com os campos obrigatórios (`stage`, `outcome`, `phase`, `scope`, `lang`, `checkpoint`). Command: `node -e "const fs=require('fs'),path=require('path');const root='.claude/spec';for(const d of fs.readdirSync(root)){const p=path.join(root,d);if(!fs.statSync(p).isDirectory())continue;const m=path.join(p,'meta.json');if(!fs.existsSync(m)){console.error('missing',m);process.exit(1)}const j=JSON.parse(fs.readFileSync(m,'utf8'));for(const k of ['stage','outcome','phase','scope','lang','checkpoint']){if(!(k in j)){console.error('missing field',k,'in',m);process.exit(1)}}}"`

- [ ] **AC-2.** O `pipeline_state_ingest` lê o `meta.json` em vez de varrer headers do `.md` quando o arquivo existe. Command: `node -e "const fs=require('fs');const t=fs.readFileSync('apps/rt/src/run/pipeline_state_ingest.rs','utf8');if(!/meta\.json/.test(t))process.exit(1)"`

- [ ] **AC-3.** Os comandos que criam specs novas (`mustard-rt run wave-scaffold`, `mustard-rt run emit-pipeline`, e o caminho de criação em `tactical-fix`) escrevem `meta.json` ao lado do `spec.md`. Command: `node -e "const fs=require('fs');const t1=fs.readFileSync('apps/rt/src/run/wave_scaffold.rs','utf8');const t2=fs.readFileSync('apps/rt/src/run/emit_pipeline.rs','utf8');if(!/write_meta/.test(t1)||!/write_meta/.test(t2))process.exit(1)"`

- [ ] **AC-4.** O dashboard lê o `meta.json` direto via comando Tauri novo (`read_spec_meta`) em vez de depender do parser do `mustard-rt`. Command: `node -e "const fs=require('fs');const t=fs.readFileSync('apps/dashboard/src-tauri/src/commands/specs.rs','utf8');if(!/read_spec_meta|meta\.json/.test(t))process.exit(1)"`

- [ ] **AC-5.** Após a migração, os arquivos `spec.md` não contêm mais os headers `### Stage:`, `### Outcome:`, `### Phase:`, `### Scope:`, `### Lang:`, `### Checkpoint:` nem `### Parent:`. Command: `node -e "const fs=require('fs'),path=require('path');const root='.claude/spec';let bad=[];for(const d of fs.readdirSync(root)){const p=path.join(root,d);if(!fs.statSync(p).isDirectory())continue;for(const f of fs.readdirSync(p)){if(!f.endsWith('.md'))continue;const txt=fs.readFileSync(path.join(p,f),'utf8');if(/^###\s+(Stage|Outcome|Phase|Scope|Lang|Checkpoint|Parent):/m.test(txt))bad.push(path.join(p,f))}}if(bad.length){console.error('headers ainda presentes em:',bad);process.exit(1)}"`

- [ ] **AC-6.** O `spec_sections.rs` foi simplificado: a tabela de variantes de heading deixa de existir para os headers `### X:` (continua só pras seções de conteúdo `## Contexto`/`## Context` etc., se ainda precisarem ser parseadas). Command: `node -e "const fs=require('fs');const t=fs.readFileSync('apps/rt/src/run/spec_sections.rs','utf8');if(/\"stage\"|\"outcome\"|\"phase\"|\"scope\"/.test(t))process.exit(1)"`

- [ ] **AC-7.** A migração one-shot rodou em todas as specs existentes — nenhuma ficou sem `meta.json`. Command: já coberto por AC-1.

- [ ] **AC-8.** O `migrate_spec_headers.rs`, que existia pra reescrever headers no `.md`, ou foi simplificado pra escrever `meta.json` ou foi deletado (a função dele saiu do jogo). Command: `node -e "const fs=require('fs');const p='apps/rt/src/run/migrate_spec_headers.rs';if(!fs.existsSync(p))process.exit(0);const t=fs.readFileSync(p,'utf8');if(/write_meta|meta\.json/.test(t))process.exit(0);process.exit(1)"`

## Plano

| Onda | Papel | O que faz | Depende |
|---|---|---|---|
| 1 | rt | Define o schema do `meta.json` em `mustard-core`. Cria leitor + escritor. Adapta `pipeline_state_ingest` pra ler JSON primeiro (com fallback pro parser antigo se o arquivo não existe — modo compat). | — |
| 2 | rt | Migração one-shot: novo subcomando `mustard-rt run migrate-to-meta` percorre todas as specs em `.claude/spec/**` e cria `meta.json` ao lado do `spec.md`, copiando os valores dos headers atuais. Headers continuam no `.md` por enquanto (espelho). | Onda 1 |
| 3 | rt + ui | Os escritores (`wave-scaffold`, `emit-pipeline`, `tactical-fix`, comandos de spec do CLI) passam a escrever `meta.json` ao criar specs novas. Dashboard ganha um comando Tauri `read_spec_meta` e lê direto. | Onda 1 |
| 4 | rt | Cleanup: remove os headers `### X:` do `.md` (passam a viver só no JSON). Simplifica `spec_sections.rs` (tira tabela de variantes pros campos de máquina). Decide o destino do `migrate_spec_headers.rs`. | Ondas 2 e 3 |

Ondas 2 e 3 podem rodar em paralelo depois da Onda 1 — não se cruzam (uma mexe em specs existentes, outra em código de escrita).

## Schema do `meta.json`

```json
{
  "stage": "Plan",
  "outcome": "Active",
  "phase": "PLAN",
  "scope": "full",
  "lang": "pt",
  "checkpoint": "2026-05-24T17:00:00Z",
  "parent": null,
  "isWavePlan": true,
  "totalWaves": 4
}
```

Campos obrigatórios: `stage`, `outcome`, `phase`, `scope`, `lang`, `checkpoint`.
Campos opcionais: `parent` (slug da spec pai, ou null), `isWavePlan` (true se a spec tem ondas), `totalWaves` (quando `isWavePlan` é true).

Valores aceitos:

- `stage`: `Plan`, `Execute`, `Review`, `QA`, `Close`.
- `outcome`: `Active`, `Completed`, `Blocked`, `Cancelled`.
- `phase`: `ANALYZE`, `PLAN`, `EXECUTE`, `REVIEW`, `QA`, `CLOSE`, `COORDINATE`.
- `scope`: `light`, `extended-light`, `full`.
- `lang`: `pt` ou `en` (mantemos o formato curto que já é usado nos headers — não é o `pt-BR`/`en-US` da spec C, que opera num nível diferente).
- `checkpoint`: ISO 8601 UTC.

## Cobertura

| Preocupação levantada | Onde resolve |
|---|---|
| Headers de spec devem ser padrão e não mudar com idioma/tom | AC-5 e AC-6 — Onda 4 elimina os headers e simplifica o parser. |
| Evitar abrir o `.md` toda vez só pra ler um campo | AC-2 e AC-4 — Ondas 1 e 3 fazem parser e dashboard lerem JSON direto. |
| `mustard:feature` e `mustard:spec` fazem insert no SQLite via parsing — precisa cobrir | AC-3 — Onda 3 atualiza os escritores. SQLite continua sendo a fonte viva, só muda a origem da leitura. |
| Project-profiler não trata isso | Confirmado na análise — sem conflito de arquivos. Pode rodar agora sem espera. |
| Spec C depende disso | Sim — spec C herda este `meta.json` como base e passa a operar em terreno simplificado. |

## Limites

Arquivos que esta spec encosta:

- `packages/core/src/meta.rs` (novo) — schema + serde.
- `apps/rt/src/run/pipeline_state_ingest.rs` — leitura via meta.json.
- `apps/rt/src/run/wave_scaffold.rs` — escrita de meta.json.
- `apps/rt/src/run/emit_pipeline.rs` — escrita de meta.json.
- `apps/rt/src/run/migrate_to_meta.rs` (novo) — migração one-shot.
- `apps/rt/src/run/migrate_spec_headers.rs` — destino decidido na Onda 4.
- `apps/rt/src/run/spec_sections.rs` — simplificação na Onda 4.
- `apps/dashboard/src-tauri/src/commands/specs.rs` — comando `read_spec_meta`.
- `apps/dashboard/src/lib/dashboard.ts` (ou onde mora a chamada Tauri) — consumir `read_spec_meta`.
- `apps/cli/templates/commands/mustard/{feature,spec,tactical-fix}/SKILL.md` — instruções dos escritores referenciam `meta.json` em vez de headers.
- `.claude/spec/**/*.md` — limpos na Onda 4 (remoção dos headers).
- `.claude/spec/**/meta.json` — criados na Onda 2.

Fora de limites:

- SQLite: nenhuma mudança de schema. Eventos `pipeline.status`/`pipeline.phase` continuam idênticos.
- Conteúdo narrativo das specs (PRD, Plano, Tarefas, AC) — não muda.
- Dashboard UI de edição de campos: fica pra spec C ou futura.

## Preocupações

- **Compatibilidade durante a transição.** Entre o fim da Onda 2 e o início da Onda 4, vai existir um período em que tanto os headers do `.md` quanto o `meta.json` carregam a mesma informação. O leitor da Onda 1 já está preparado pra preferir o JSON, então isso é seguro — mas qualquer edição manual nesse período precisa atualizar ambos. Não é o caso normal (o pipeline edita via emit-pipeline), mas vale a nota.
- **Migração de specs muito antigas.** Algumas specs no histórico podem ter headers em formato variante (faltando algum campo, ou em PT). O migrador da Onda 2 precisa ser tolerante: se faltar `phase`, deriva de `stage`; se faltar `lang`, assume `pt`; etc.
