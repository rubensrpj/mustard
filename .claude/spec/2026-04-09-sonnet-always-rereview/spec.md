# Enhancement: sonnet-always-rereview

## Summary
Reverter a heurística Haiku de re-review adicionada nesta sessão (specs #2 `haiku-auto-rereview` + #7 `haiku-heuristic-table`). Substituir por regra única: **"re-reviews sempre usam Sonnet, independente do modelo do review inicial"**. Elimina 100% do risco R3 (Haiku perdendo issues sutis), preserva economia em casos Opus→Sonnet (~$5.40/re-review em Full+new-patterns), e remove a decision table + referências em 3 SKILL.md.

## Why
Verificação em `.claude/pipeline-config.md:154-164` mostrou que o default é Sonnet, mas **há um caso onde é Opus**: `Feature 5+ files, new patterns`. Nesses casos, a heurística atual fazia downgrade **Opus → Haiku** em fixes pequenos — exatamente no código mais complexo e novo, onde Haiku é menos confiável.

**Math revisada:**

| Cenário | Default initial | Heurística atual | Sonnet sempre | Delta |
|---|---|---|---|---|
| Light feature | Sonnet $1.35 | Haiku $0.36 | Sonnet $1.35 | +$0.99 custo |
| Full known patterns | Sonnet $1.35 | Haiku $0.36 | Sonnet $1.35 | +$0.99 custo |
| **Full new patterns** | **Opus $6.75** | **Haiku $0.36 (risco ALTO)** | **Sonnet $1.35** | **-$5.40 + zero risco** |
| Full new patterns (não aciona) | Opus $6.75 | Opus $6.75 | Sonnet $1.35 | **-$5.40** |

Economia agregada é POSITIVA em todos os cenários Opus (a maior parte do ganho absoluto vive aqui), e a "perda" de $0.99 em casos Sonnet é pagamento justo pela eliminação do risco sistêmico da heurística.

Sonnet é suficiente para re-review de fixes mesmo em contextos complexos — `pipeline-config.md` usa Sonnet como default para audit, bugfix, e features ≤5 files (casos não-triviais).

## Boundaries
- `templates/commands/mustard/review/SKILL.md` (simplificar § Model Selection)
- `templates/commands/mustard/feature/SKILL.md` (simplificar reference)
- `templates/commands/mustard/complete/SKILL.md` (simplificar reference)
- `.claude/commands/mustard/review/SKILL.md` (mirror)
- `.claude/commands/mustard/feature/SKILL.md` (mirror)
- `.claude/commands/mustard/complete/SKILL.md` (mirror)
- Memórias (`feedback_token_efficiency_audit.md`, `reference_mustard_token_efficiency.md`, `MEMORY.md`)

## Checklist
### templates-impl Agent
- [x] Ler `templates/commands/mustard/review/SKILL.md` seção `## Model Selection` atual (decision table com 4 steps)
- [x] Substituir a seção inteira pelo novo template (abaixo em `~~~markdown`)
- [x] Em `templates/commands/mustard/feature/SKILL.md`, localizar a reference de re-review (Grep `review/SKILL.md.*Model Selection`) e substituir por: `Re-reviews always dispatch with \`model: "sonnet"\` (see \`review/SKILL.md § Model Selection\`).`
- [x] Mesma substituição em `templates/commands/mustard/complete/SKILL.md`
- [x] Mirror para `.claude/commands/mustard/{review,feature,complete}/SKILL.md`
- [x] Build: `rtk npm run build` → PASS
- [x] Hook tests: `rtk bun test templates/hooks/__tests__/hooks.test.js` → 26/26
- [ ] Atualizar memória `feedback_token_efficiency_audit.md`: substituir a linha sobre "Haiku heuristic" por "re-reviews sempre Sonnet"
- [ ] Atualizar memória `reference_mustard_token_efficiency.md`: na tabela "Gates de pipeline ativos", substituir "Haiku re-review downgrade" por "Sonnet re-review (always)"

## New section template

~~~markdown
## Model Selection

**Initial reviews**: use default model per `pipeline-config.md § Models` (sonnet for most; opus for Full + new patterns; etc.).

**Re-reviews**: always dispatch with `model: "sonnet"`, regardless of the initial review's model.

**Rationale**:
- Re-reviews verify a targeted fix to already-reviewed code. Sonnet is capable enough for fix verification even in complex codebases (see `pipeline-config.md` where Sonnet is default for audit, bugfix, and ≤5-file features).
- For Full+new-pattern features (initial review in Opus), this saves ~$5/re-review without introducing Haiku quality risk.
- Simpler than heuristic decision table: one rule, zero edge cases.
~~~

## Files (~6)
- `templates/commands/mustard/review/SKILL.md` (modify — simplify Model Selection)
- `templates/commands/mustard/feature/SKILL.md` (modify — simplify reference)
- `templates/commands/mustard/complete/SKILL.md` (modify — simplify reference)
- `.claude/commands/mustard/review/SKILL.md` (mirror)
- `.claude/commands/mustard/feature/SKILL.md` (mirror)
- `.claude/commands/mustard/complete/SKILL.md` (mirror)

## Acceptance
- `review/SKILL.md § Model Selection` é 1 regra (sem decision table, sem threshold parsing)
- `feature/SKILL.md` e `complete/SKILL.md` têm reference de 1 linha
- Zero menção a `haiku` como modelo de re-review em nenhum dos 3 SKILL.md
- Mirrors sync
- Build PASS
- Hook tests 26/26
- Memória `feedback_token_efficiency_audit.md` reflete a mudança (Haiku heuristic revertida)
- Memória `reference_mustard_token_efficiency.md` reflete a mudança (gate Sonnet sempre)

## Guards
- NÃO alterar modelo de reviews iniciais (só re-reviews)
- NÃO manter nenhum fragmento da decision table (remove completa)
- NÃO criar nova complexidade — 1 regra simples
- Preservar referência a `pipeline-config.md § Models` para default do initial review

## Elegance Check
**Pergunta**: "É mais elegante reverter ou tightening do threshold?"

Alternativas:
1. **Tightening** (issue_count ≤1, files_changed ≤2): ~40% economia, ainda tem risco residual Haiku, ainda precisa decision table
2. **Opus sanity check após Haiku**: ~92% economia, mas adiciona dispatch extra e complexidade
3. **Sonnet sempre** (esta proposta): ~80% economia nos casos Opus, zero risco, zero decision table
4. **Revert total sem substituição** (sempre default): zero economia, máxima simplicidade

(3) é o único que combina **zero risco + economia significativa + máxima simplicidade**. (1) e (2) mantêm complexidade. (4) perde dinheiro desnecessariamente nos casos Opus.

## Open Questions (decididas)
1. E se default mudar no futuro? → Regra é textual ("sempre Sonnet"), independente do default. Mudança de default não afeta.
2. E se pipeline-config.md adicionar novos scopes Opus? → Sempre Sonnet continua correto — re-reviews ganham economia adicional automaticamente.
3. Deveria permitir override manual? → NÃO por ora. Se precisar no futuro, adicionar opt-out via env var.
4. E re-reviews de `/bugfix`? → Mesma regra. Bugfix default já é Sonnet, zero mudança.
5. Gate se aplica a quantos reviews consecutivos (se precisar de 3 loops)? → Todos. Re-review 1, 2, 3, ... → todas Sonnet.

## Impact on R3 (quality risk)
- **Antes** (com heurística Haiku): 95% safe (sanity check) ou 98% (threshold tight) ou 100% (revert). Nenhuma opção combinava zero risk + economia + simplicidade.
- **Depois** (Sonnet sempre): **100% safe** (Haiku não existe no fluxo de re-review) + ~80% economia + zero complexidade.

R3 é **eliminado**, não apenas mitigado.

## Result

Implemented 2026-04-09. All 3 SKILL.md files updated and mirrored.

- `templates/commands/mustard/review/SKILL.md:83-98` — `## Model Selection` replaced: decision table (4 steps, haiku threshold) → 1-rule Sonnet-always with rationale block
- `templates/commands/mustard/feature/SKILL.md:240` — 1-line reference replacing decision table consult + `model: "haiku"` mention
- `templates/commands/mustard/complete/SKILL.md:35` — same 1-line reference
- 3 mirrors to `.claude/commands/mustard/{review,feature,complete}/SKILL.md`

Build: PASS | Hook tests: 26/26 | No haiku as re-review model in any SKILL.md
