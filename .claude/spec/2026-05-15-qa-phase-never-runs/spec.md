# Bugfix: QA nunca executa em pipelines Full/wave

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: full
### Checkpoint: 2026-05-15T19:48:42Z
### Lang: pt

## Contexto

O pipeline Mustard define a fase QA (Wave 10) como o contrato Dev/QA: depois que o EXECUTE termina, o `qa-run.js` executa cada Acceptance Criteria da spec e só então o pipeline pode fechar. Na prática isso não acontece. O comando `/resume` — que orquestra todo pipeline Full scope e todo wave plan — vai direto de REVIEW para CLOSE, sem nenhum passo de QA. O `/feature` tem uma seção "QA Phase" escrita, mas o fluxo numerado do EXECUTE Light pula essa seção e fecha logo após o review. O resultado é que projetos reais (observado em sialia) fecham o pipeline sem rodar nenhum AC e sem gerar relatório de QA.

A rede de segurança que deveria impedir isso — o hook `close-gate.js` — também está inativa. Seu gatilho lê o campo `phase` esperando uma string, mas os arquivos pipeline-state usam `phase` como índice numérico e guardam o nome da fase em `phaseName`. Assim o gate nunca reconhece a transição para CLOSE e nunca verifica o evento `qa.result`. Os testes da Wave 10 ficaram verdes porque a fixture de teste grava `phase` como string — um formato que a produção nunca gera. O efeito combinado é um pipeline que promete validação de QA e na prática nunca a executa.

## Causa raiz

- `resume/SKILL.md` § Step 4: sequência `18 VALIDATE → 19 REVIEW → 19b Fix Loop → 20 CLOSE`, sem passo de QA. `/resume` fecha todo pipeline Full/wave.
- `feature/SKILL.md`: a seção `### QA Phase (Wave 10)` existe, mas o passo 10 do EXECUTE Light manda ir direto para "CLOSE flow inline".
- `close-gate.js` `extractPhase()`: lê `obj.phase` exigindo `typeof === 'string'`; o pipeline-state real tem `phase` numérico + `phaseName` string → retorna `null` → o gate sai sem checar QA/build/test/checklist.
- `qa-run.js`: nas saídas antecipadas "sem seção AC" e "sem itens AC" retorna `overall: 'skip'` sem emitir `qa.result`; após corrigir o gatilho, o close-gate veria "nenhum QA" e bloquearia indevidamente specs sem AC.
- `harness-wave10.test.js`: o helper `makePipelineStateInput` grava `phase` como string — divergente do artefato real — mascarando o gatilho morto.

## Plano

1. **`close-gate.js` — `extractPhase()`**: ler `phaseName` (string) como campo primário, com fallback para `phase` string (compat com fixtures legados). Retornar `null` se nenhum dos dois for string.
2. **`close-gate.js` — gate de QA**: tratar `overall === 'skip'` como liberado (warn em stderr, fall-through), alinhado a `/mustard:qa § Step 5`. Só `overall` ausente (`!found`) ou `=== 'fail'` continua negando em modo strict.
3. **`qa-run.js`**: emitir `qa.result` com `overall: 'skip'` e `criteria: []` também nas saídas "sem seção AC" e "sem itens AC parseável", para o close-gate ter sempre um evento a consumir. As saídas "spec não encontrada"/"erro de leitura" continuam sem emitir (erro de ambiente, não veredito de QA).
4. **`resume/SKILL.md`**: inserir `### Step 19c: QA Phase (Wave 10) — MANDATORY` entre o Fix Loop (19b) e o `20. CLOSE`. Conteúdo: seta `phaseName: "QA"` no pipeline-state; roda `bun .claude/scripts/qa-run.js --spec {specName}`; ramifica `pass` (atualiza checkboxes de AC, escreve `phaseName: "CLOSE"` via Write/Edit — isso dispara o `close-gate.js`, segue para Step 20) / `fail` (devolve a lista de AC falhos ao fix-loop do Step 19b, re-executa, máx. 3 iterações) / `skip` (avisa inline e segue para Step 20). Adicionar à seção INVIOLABLE RULES: "ALWAYS run QA (Step 19c) após REVIEW e antes de CLOSE — nunca ir REVIEW→CLOSE direto".
5. **`feature/SKILL.md`**: o passo 10 do EXECUTE Light passa a rotear por QA antes do CLOSE (`All passed + APPROVED → QA Phase (Wave 10) → on QA pass/skip → CLOSE flow inline`). A seção `### QA Phase (Wave 10)` passa a instruir, no `pass`, a escrita de `phaseName: "CLOSE"` no pipeline-state (dispara o close-gate).
6. **`harness-wave10.test.js`**: `makePipelineStateInput` passa a gerar o formato real (`phaseName` string + `phase` numérico dummy). Novos testes: (a) gatilho dispara com `{ phase: 3, phaseName: "CLOSE" }` + sem `qa.result` → deny; (b) `qa.result overall=skip` → CLOSE liberado; (c) `qa-run` emite `qa.result overall=skip` quando a spec não tem seção AC.

### Achados durante o EXECUTE (mesma direção — corrigidos)

7. **`harness-wave10.test.js` — fixture quebrada**: `EXIT_PASS`/`EXIT_FAIL` usavam `cmd /c exit N`. O `qa-run` resolve o Git Bash como shell no Windows, e o MSYS converte o argumento `/c` em path → o `cmd` abre uma sessão interativa e sai 0. Os testes "AC fail → overall=fail" nunca falhavam de fato. Trocado por `node -e "process.exit(N)"` (imune a path-mangling, cross-shell).
8. **`qa-run.js` — heading de AC só em inglês**: `extractACSection` só reconhecia `## Acceptance Criteria`. Specs com `Lang: pt` usam `## Critérios de Aceitação` (HARD RULE do `feature`/`bugfix` SKILL) → QA dava SKIP em **todo projeto pt-BR** (causa direta do observado em sialia). O regex do heading passa a reconhecer os dois headings canônicos.

## Limites

- `templates/hooks/close-gate.js`
- `templates/scripts/qa-run.js`
- `templates/commands/mustard/resume/SKILL.md`
- `templates/commands/mustard/feature/SKILL.md`
- `templates/hooks/__tests__/harness-wave10.test.js`

## Preocupações

- `/complete` (finalize manual) não tem passo de QA próprio, e o `close-gate` não dispara nele — o `complete-spec.js` grava o pipeline-state via `fs.writeFileSync` interno, fora do hook `PreToolUse`. Aceitável: `/complete` roda após o QA já feito por `resume`/`feature`. Não corrigido nesta spec — fora de escopo, registrado para decisão futura.
- O próprio repo `mustard` faz dogfooding e tem cópias instaladas defasadas em `.claude/scripts/` e `.claude/hooks/` (ex.: `.claude/scripts/qa-run.js` ainda com o regex inglês). A correção vive em `templates/` (fonte); rodar `bun bin/mustard.js update` propaga para o `.claude/` deste repo. Fora do escopo desta spec (Boundaries = `templates/`).

## Critérios de Aceitação

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: Testes do hook da Wave 10 (qa-run + close-gate) passam — Command: `bun test templates/hooks/__tests__/harness-wave10.test.js`
- [x] AC-2: `resume/SKILL.md` define uma fase QA entre REVIEW e CLOSE — Command: `node -e "const s=require('fs').readFileSync('templates/commands/mustard/resume/SKILL.md','utf8');const a=s.indexOf('Step 19c');const b=s.indexOf('20. **CLOSE');process.exit(a>-1&&b>-1&&a<b?0:1)"`
- [x] AC-3: passo 10 do EXECUTE Light em `feature/SKILL.md` roteia por QA antes do CLOSE — Command: `node -e "const s=require('fs').readFileSync('templates/commands/mustard/feature/SKILL.md','utf8');const l=s.split('\n').find(x=>/^10\. /.test(x))||'';process.exit(/QA/.test(l)?0:1)"`
- [x] AC-4: suíte de regressão dos hooks continua verde — Command: `bun test templates/hooks/__tests__/hooks.test.js`
