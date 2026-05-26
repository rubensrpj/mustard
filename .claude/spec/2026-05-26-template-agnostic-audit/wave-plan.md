# Plano de Waves

### Stage: Plan
### Outcome: Active
### Flags: 
### Scope: full (wave plan)
### Total waves: 7

## Tabela de Waves

| Wave | Spec | Role | Depende de | Resumo |
|------|------|------|------------|--------|
| 1 | [[wave-1-cli]] | cli | — | Purge templates/recipes/. Apagar add-field/add-endpoint/add-component/add-validation/null-guard.json. Init para de instalar skeletons CRUD. Validar que recipe-match degrada silencioso (exit 0) quando nao acha recipe. |
| 2 | [[wave-2-cli]] | cli | [[wave-1-cli]] | Rewrite templates/CLAUDE.md (meta-agnostico, zero stack do Mustard) + sanitize templates/pipeline-config.md (remover roles fixos api/mobile/ui/database/library, Flutter->mobile default, placeholders backend/frontend/admin, default DB+Backend Wave 1 / Frontend Wave 2). |
| 3 | [[wave-3-cli]] | cli | [[wave-2-cli]] | Mover templates/refs/stack-templates/ (fe-craft-check.md, browser-debug.md) para templates-extras/refs/stack-templates/ (opt-in mesmo padrao do hallmark). Re-classificar skill:react-best-practices em .artifacts.json como opt-in. Init para de instalar artefatos UI-especificos por default. |
| 4 | [[wave-4-rt]] | rt | [[wave-1-cli]] | Purge apps/rt/src/run/recipe_match.rs: remover find_dir_by_convention, resolve_pattern, to_pascal_case e hardcode de backend/frontend/admin. Recipe derivado do scan ja vem com path real. Avaliar remocao do arquivo inteiro se nada mais consumir. scan-recipes-validate continua passando. |
| 5 | [[wave-5-mixed]] | core | [[wave-2-cli]] | **Refator i18n: split de tipos.** Hoje Locale (em packages/core/src/i18n.rs) faz dois trabalhos misturados: (1) catalogo de strings que o Mustard sabe traduzir, (2) idioma da spec/projeto. Confusao impede user pedir specLang="fr-FR" (toma LocaleError::Unknown). Renomear Locale->SupportedLocale (catalogo fechado, hoje PtBr+EnUs). Criar UserLocale(String) newtype BCP-47 sintatico (xx-YY), aceita qualquer codigo valido. Helper UserLocale::to_supported -> Option<SupportedLocale>. translate() e apply_tone() so aceitam SupportedLocale por construcao. Callsites decidem qual tipo cabe. Sem mudar comportamento user-visivel ainda. |
| 6 | [[wave-6-mixed]] | mixed | [[wave-5-mixed]] | Recipe concept death: pasta .claude/recipes/ ja deletada manualmente (W4 cleanup); agora matar o conceito por completo. DELETE: apps/rt/src/run/recipe_match.rs, apps/rt/src/run/scan_recipes_validate.rs. REMOVE: variantes RunCmd::RecipeMatch + ScanRecipesValidate no enum, dispatch em mod.rs, SavingsSource::RecipeInjection no economy enum (tipo morto). PURGE: kind "recipe" do scan taxonomy (interpret.rs, graph.rs, resolve.rs docstrings + seed *.recipe.<slug>), validation contract em scan_md_validate.rs, chamada recipe-match em templates/commands/mustard/feature/SKILL.md e task/SKILL.md, menções em pipeline-config.md e browser-debug.md. Verificar dashboard/lib + packages/core por refs residuais. ~20 arquivos. |
| 7 | [[wave-7-mixed]] | mixed | [[wave-5-mixed]] | **Policy enforcement (resto da W5 original).** Apos refator i18n: (a) Sweep `== "pt"` / `== "en"` -> `== "pt-BR"` / `== "en-US"` em apps/rt/src/ + apps/dashboard/src-tauri/ + tests. mustard.json#specLang vira pt-BR; adicionar tone:"didactic". Dashboard i18n type Lang = "pt-BR" \| "en-US". (b) Tone wire: spec_draft.rs le tone do mustard.json, injeta no prompt do agente drafter. Sem heuristica nova. (c) language-audit subcomando: escaneia targets pre-definidos (templates/, apps/*/src/, packages/*/src/, .claude/refs/, .claude/commands/, .claude/skills/) por arquivos com palavras PT distintivas (com diacritico) acima de threshold. Soft warning, exit 0. (d) REWRITE templates/refs/feature/spec-language.md cobrindo 3 dimensoes (idioma spec, tone, resto EN). MODIFY SKILLs feature/bugfix do template + espelho local. ~22 arquivos. |
