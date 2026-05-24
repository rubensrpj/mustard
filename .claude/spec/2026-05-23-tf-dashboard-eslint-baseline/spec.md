# Tactical-fix — dashboard ESLint baseline (flat config + zero violações)

## Resumo

O `apps/dashboard/` declara `"lint": "eslint ."` em `package.json` mas o repo nunca teve um `eslint.config.js` commitado. ESLint v9 exige flat config — sem ele, `pnpm --filter mustard-dashboard lint` aborta antes de avaliar qualquer regra. Wave 1 do parent (`2026-05-23-dashboard-design-system`) flagou isso como CONCERN pré-existente: não é regressão da Wave 1, mas trava AC-W1.6 (lint passa) e AC-2 (parent: dashboard lint passa após refactor). Esta tactical-fix instala o flat config mínimo + plugins React/TS + baseline as warnings (não erros) e fecha apontamentos triviais até `pnpm lint` rodar exit 0 numa primeira passagem.

## Arquivos

- `apps/dashboard/eslint.config.js` (NOVO — flat config exportando `default` array)
- `apps/dashboard/package.json` (EDIT — adicionar devDeps: `eslint ^9`, `@typescript-eslint/eslint-plugin`, `@typescript-eslint/parser`, `eslint-plugin-react`, `eslint-plugin-react-hooks`, `eslint-plugin-react-refresh`, `globals`)
- `pnpm-lock.yaml` (EDIT — atualizado por `pnpm install`)

## Decisão de severidade

Baseline pré-existente significa "violações desconhecidas em N arquivos". Para evitar arrastar este TF por horas, a configuração inicial:

1. Liga **regras críticas** como `error` (no-undef, no-unused-vars em variáveis não-`_`, react-hooks/rules-of-hooks).
2. Liga **regras estilísticas** como `warn` (any explícito, console.log, prefer-const).
3. Roda `pnpm lint` — se errors > 0, corrige só os critical errors. Warnings ficam para um segundo TF se virarem dor.

## Tarefas

- [ ] **package.json**: adicionar devDeps listadas acima. Rodar `pnpm install` para baixar.
- [ ] **eslint.config.js**: criar flat config em `apps/dashboard/eslint.config.js` cobrindo `.ts`, `.tsx`. Estrutura mínima:
  ```js
  import js from '@eslint/js'
  import tseslint from '@typescript-eslint/eslint-plugin'
  import tsParser from '@typescript-eslint/parser'
  import react from 'eslint-plugin-react'
  import reactHooks from 'eslint-plugin-react-hooks'
  import reactRefresh from 'eslint-plugin-react-refresh'
  import globals from 'globals'

  export default [
    js.configs.recommended,
    {
      files: ['src/**/*.{ts,tsx}'],
      languageOptions: { parser: tsParser, globals: { ...globals.browser, ...globals.es2022 } },
      plugins: { '@typescript-eslint': tseslint, react, 'react-hooks': reactHooks, 'react-refresh': reactRefresh },
      settings: { react: { version: '19' } },
      rules: {
        ...tseslint.configs.recommended.rules,
        ...react.configs.recommended.rules,
        ...reactHooks.configs.recommended.rules,
        'react/react-in-jsx-scope': 'off',
        'react/prop-types': 'off',
        '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_', varsIgnorePattern: '^_' }],
        '@typescript-eslint/no-explicit-any': 'warn',
        'no-console': ['warn', { allow: ['warn', 'error'] }],
      },
    },
    { ignores: ['dist', 'src-tauri', 'public', '.tauri', 'node_modules', '*.config.{js,ts}'] },
  ]
  ```
- [ ] **lint primeira passagem**: `pnpm --filter mustard-dashboard lint` → contar errors vs warnings.
- [ ] **Triagem errors**: se >0 errors críticos, corrigir um a um (commits granulares). Se warnings, deixar para próximo TF.
- [ ] **Reapontar AC parent**: editar `.claude/spec/2026-05-23-dashboard-design-system/spec.md` AC-2 e `wave-1-general/spec.md` AC-W1.6 trocando "DEFERRED → [[2026-05-23-tf-dashboard-eslint-baseline]]" por "[x] passa via TF dashboard-eslint-baseline".

## Acceptance Criteria

- [ ] AC-TF-E-1: `apps/dashboard/eslint.config.js` existe e exporta default array — Command: `node -e "import('./apps/dashboard/eslint.config.js').then(m=>{if(!Array.isArray(m.default))process.exit(1);console.log('ok')}).catch(e=>{console.error(e);process.exit(2)})"`
- [ ] AC-TF-E-2: `pnpm --filter mustard-dashboard lint` exit 0 — Command: `pnpm --filter mustard-dashboard lint`
- [ ] AC-TF-E-3: devDeps eslint v9 + plugins presentes em `apps/dashboard/package.json` — Command: `node -e "const p=require('./apps/dashboard/package.json');const d=p.devDependencies||{};const need=['eslint','@typescript-eslint/parser','@typescript-eslint/eslint-plugin','eslint-plugin-react','eslint-plugin-react-hooks','globals'];const miss=need.filter(n=>!d[n]);if(miss.length){console.error('missing:',miss);process.exit(1)}console.log('ok')"`

## Limites

- `apps/dashboard/eslint.config.js` (novo)
- `apps/dashboard/package.json`
- `pnpm-lock.yaml`
- Arquivos `.ts/.tsx` em `apps/dashboard/src/` SE e SOMENTE SE forem critical errors do lint (não warnings)

OUT: tudo fora dessa lista. Tocar em código de outras subprojects (rt/core/cli) é violação de boundary.

## Modelo

sonnet (tactical fix, escopo bem delimitado; sem decisões de design)
