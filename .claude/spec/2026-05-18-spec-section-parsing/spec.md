# Feature: spec-section-parsing

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: full
### Checkpoint: 2026-05-18T21:51:01Z
### Lang: pt

## Contexto

O Mustard gera cada arquivo de spec num idioma escolhido uma vez por projeto — português ou inglês — e essa escolha já é resolvida e guardada de forma central (campo `specLang`, gravado pela própria ferramenta no primeiro uso). Quando o idioma é português, a spec inteira é escrita em português, inclusive os cabeçalhos de cada seção: a seção de arquivos vira "Arquivos", a de tarefas vira "Tarefas", e assim por diante.

Por baixo, vários scripts internos da ferramenta precisam ler trechos da spec — por exemplo, encontrar a lista de arquivos para validar limites, ou a lista de critérios de aceitação para rodar o QA. Esses scripts foram escritos esperando os cabeçalhos sempre em inglês. Numa spec em português eles simplesmente não encontram a seção que procuram e desistem em silêncio: o QA é pulado, validações de fase não rodam, gates de fechamento não enxergam pendências. O usuário não vê erro — só perde a proteção.

Hoje isso foi remendado num único script (o do QA ganhou um padrão que aceita os dois idiomas, escrito à mão na linha dele). Os outros oito continuam quebrados, cada um com sua própria string fixa em inglês. O remendo espalhado é frágil e some quando ninguém olha. Esta spec elimina a classe inteira do problema: cria uma fonte única que sabe traduzir cabeçalho ↔ idioma, e faz todos os scripts passarem por ela.

## Resumo

Centralizar o reconhecimento de cabeçalhos de seção de spec num único módulo e fazer todos os parsers consumirem esse módulo, tornando-os idioma-agnósticos:

- Criar `templates/scripts/_lib/spec-sections.js` — a "Header Translation Table" de `spec-language.md` transcrita para código, com helpers para localizar uma seção por chave canônica em qualquer idioma.
- Refatorar os ~9 scripts/hooks que hoje hardcodam o cabeçalho em inglês para usar o módulo — comportamento para specs `en` permanece idêntico, specs `pt` passam a ser lidas corretamente.
- Atualizar `spec-language.md` para apontar o módulo como a fonte canônica em código (o markdown vira documentação, não a verdade duplicada).

Specs legadas com cabeçalhos misturados também passam a funcionar, porque o módulo tenta todas as variantes conhecidas de cada seção.

## Entidades

N/A — refatoração de tooling interno. Não há entidades de domínio, schema, endpoint ou UI envolvidos.

## Arquivos (~13)

Novos:
- `templates/scripts/_lib/spec-sections.js` — módulo de reconhecimento de seções
- `templates/hooks/__tests__/spec-sections.test.js` — testes do módulo

Refatorados (trocam string/regex literal pelo módulo):
- `templates/scripts/spec-extract.js` — `extractAcceptanceCriteria` (heading hardcoded EN, linha ~115)
- `templates/scripts/exec-rewave-check.js` — parser de `## Files` (linhas ~29, ~195; foi quem retornou `no-files-section` numa spec pt)
- `templates/scripts/qa-run.js` — remover o regex bilíngue inline (linha ~74) e usar o módulo
- `templates/scripts/analyze-validation.js` — parser de `## Files` (linha ~46)
- `templates/scripts/pipeline-summary.js` — acesso a `sections['Acceptance Criteria']` (linha ~206)
- `templates/scripts/wave-size-check.js` — parser de `## Files` (linha ~182)
- `templates/scripts/_lib/wave-lib.js` — parser de `## Files` (linha ~26)
- `templates/hooks/boundary-gate.js` — parser de `## Files` / `## Boundaries` (linhas ~104-105)
- `templates/hooks/close-gate.js` — `ACTIONABLE_SECTIONS` Tasks/Checklist/Acceptance (linha ~110)
- `templates/hooks/spec-size-gate.js` — heading `## Acceptance Criteria` (linha ~49)
- `templates/hooks/guard-verify.js` — heading `## Boundaries` (linha ~182)

Documentação:
- `templates/refs/feature/spec-language.md` — nota apontando o módulo como fonte canônica

## Limites

- `templates/scripts/` e `templates/scripts/_lib/` — módulo novo + parsers refatorados
- `templates/hooks/` — hooks refatorados + teste novo
- `templates/refs/feature/spec-language.md` — apenas a nota de fonte canônica
- **Fora dos limites:** `src/`, `dist/`, o espelho `.claude/` do próprio repo, `templates/spec/` (specs históricas). Comportamento para specs `Lang: en` deve permanecer byte-idêntico.

## Tarefas

### Templates Agent (Wave 1) — módulo de fonte única

- [x] Criar `templates/scripts/_lib/spec-sections.js` (CommonJS, somente built-ins do Node, padrão dos outros `_lib/`). Exportar:
  - `SECTIONS` — mapa de chave canônica → variantes de idioma, transcrito da Header Translation Table de `spec-language.md` (`files: ['Files','Arquivos']`, `tasks: ['Tasks','Checklist','Tarefas']`, `acceptanceCriteria: ['Acceptance Criteria','Critérios de Aceitação']`, `boundaries: ['Boundaries','Limites']`, `summary: ['Summary','Resumo']`, `context`, `rootCause`, `plan`, `nonGoals`, `concerns`, `decisions`, `dependencies`, `entityInfo`, `symptom`).
  - `headingRegex(key)` — `RegExp` que casa `^##\s+(<variantes alternadas e escapadas>)\b...` case-insensitive/multiline; tolera sufixo após o nome (ex.: `## Acceptance Criteria (this pipeline)`).
  - `findSection(markdown, key)` — retorna `{ start, end, content }` (do cabeçalho até o próximo `## ` ou fim) ou `null`.
  - `sectionHeading(key, lang)` — devolve a string de cabeçalho correta para o gerador escrever (`lang` ∈ `pt|en`, default `en`).
- [x] Criar `templates/hooks/__tests__/spec-sections.test.js` — cobrir: cada chave em `pt` e `en`; seção ausente → `null`; seção no fim do arquivo (sem `## ` seguinte); cabeçalho com sufixo entre parênteses; `sectionHeading` nos dois idiomas.
- [x] Rodar `bun test templates/hooks/__tests__/spec-sections.test.js` — deve passar.
- [x] Rodar `npm run build` — deve passar.

### Templates Agent (Wave 2) — refatorar parsers

- [x] Refatorar cada parser listado em `## Arquivos` para localizar a seção via `headingRegex`/`findSection` do módulo, substituindo a string/regex literal em inglês. Regra: o comportamento para specs `Lang: en` deve ficar idêntico; o objetivo é apenas passar a reconhecer também os cabeçalhos `pt`. Em `qa-run.js`, remover o regex bilíngue inline e delegar ao módulo (sem mudança de comportamento).
- [x] Estender os arquivos de teste existentes dos parsers críticos com um caso `pt`: `exec-rewave-check.test.js` (spec com `## Arquivos` → não retorna `no-files-section`), e os testes de `boundary-gate` e `close-gate` (specs com `## Limites` / `## Tarefas` / `## Critérios de Aceitação` reconhecidas).
- [x] Atualizar `templates/refs/feature/spec-language.md` — adicionar nota na Header Translation Table indicando que a fonte canônica em código é `scripts/_lib/spec-sections.js` (o markdown documenta, o módulo é a verdade).
- [x] Rodar `bun test templates/hooks/` (suíte completa de hooks) — deve passar.
- [x] Rodar `npm run build` final — deve passar.

## Dependências

- Wave 2 depende de Wave 1 (os parsers importam o módulo criado na Wave 1).
- Sem mudanças em `src/` — a resolução de `specLang` já existe e não é tocada.

## Acceptance Criteria

Critérios testáveis e binários (pass/fail). Cada um executável e independente, a partir da raiz do projeto.

- [x] AC-1: O módulo existe e exporta a API esperada — Command: `node -e "const m=require('./templates/scripts/_lib/spec-sections.js');['findSection','headingRegex','sectionHeading','SECTIONS'].forEach(k=>{if(m[k]===undefined){console.error('export ausente: '+k);process.exit(1)}})"`
- [x] AC-2: `findSection` reconhece a seção em inglês E em português — Command: `node -e "const {findSection}=require('./templates/scripts/_lib/spec-sections.js');const en=findSection('# S\n## Acceptance Criteria\n- [ ] a\n## Next\nx\n','acceptanceCriteria');const pt=findSection('# S\n## Critérios de Aceitação\n- [ ] a\n## Next\nx\n','acceptanceCriteria');if(!en){console.error('EN falhou');process.exit(1)}if(!pt){console.error('PT falhou');process.exit(1)}"`
- [x] AC-3: A suíte de testes do módulo passa — Command: `bun test templates/hooks/__tests__/spec-sections.test.js`
- [x] AC-4: A suíte completa de hooks passa após a refatoração dos parsers — Command: `bun test templates/hooks/`
- [x] AC-5: Build e type-check passam — Command: `npm run build`

## Não-Objetivos

- Não recriar a resolução de `specLang` — a cascata documentada em `spec-language.md` (header da spec → `mustard.json#specLang` → pergunta única) já persiste a escolha de idioma no nível do projeto. Esta spec conserta apenas o **consumo**, não a definição.
- Não adicionar pergunta de idioma ao wizard `mustard init` (`src/`) — a cascata já cobre o caso; um wizard explícito seria add-on de outra spec.
- Não mudar a HARD RULE de `spec-language.md` (Lang=pt → todos os cabeçalhos `## ` em pt) — a regra está correta; o defeito é os parsers não a respeitarem.
- Não traduzir specs históricas em `templates/spec/` nem editar à mão o espelho `.claude/` do repo (regenerado por `mustard update`).
- Não alterar o comportamento de leitura para specs `Lang: en` — deve permanecer idêntico.
