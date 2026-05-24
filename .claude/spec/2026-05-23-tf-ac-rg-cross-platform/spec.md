# Tactical Fix: ACs cross-platform sem dependência de `rg`

## Contexto

Tactical fix derivado de [[2026-05-23-dashboard-design-system]] (Wave 4 REVIEW WARNING).

ACs do parent (AC-4, AC-5) e da wave-4 (AC-W4-6, AC-W4-8, AC-W4-9) chamam `execSync('rg ...')` direto. Em ambiente Windows sem `rg` no PATH (cenário default do mustard init — vide memory `feedback_rtk_windows.md`), o comando falha com exit 1 e o `catch (e.status===1)` é interpretado erroneamente como "no matches" → AC retorna PASS falso. Em Linux/macOS com `rg` instalado o comportamento original (PASS quando realmente sem matches) muda para FAIL com falsos positivos.

Solução: substituir cada `execSync('rg ...')` por um walker Node-native (`fs.readdirSync` recursivo + regex inline) que roda em qualquer plataforma sem dependência externa. Mantém o contrato dos ACs (exit 0 ou 1, mesma mensagem de erro). Sem mudança de pattern, sem mudança de paths.

## Critérios de Aceitação

- [x] AC-TF-1: ACs reescritos rodam em ambiente sem `rg` instalado e retornam exit 0 quando NÃO há matches reais — Command: `node -e "const {execSync}=require('child_process');const sources=['apps/dashboard/src','apps/dashboard/.claude'];for(const p of sources){if(!require('fs').existsSync(p)){console.error('source missing:',p);process.exit(1)}}console.log('ok')"`
- [x] AC-TF-2: ACs reescritos retornam exit 1 quando há matches reais (verificar manualmente injetando match temporário; sem command automatizado nesta sub-spec) — Validação: revisor confere lógica do walker (`if(hits.length){process.exit(1)}`)
- [x] AC-TF-3: Nenhum AC do dashboard-design-system spec/wave-N usa mais `rg` direto — Command: `node -e "const fs=require('fs');const p=require('path');const dir='.claude/spec/2026-05-23-dashboard-design-system';const hits=[];function walk(d){for(const e of fs.readdirSync(d,{withFileTypes:true})){const f=p.join(d,e.name);if(e.isDirectory())walk(f);else if(e.name==='spec.md'){const c=fs.readFileSync(f,'utf8');if(/execSync\('rg /.test(c))hits.push(f);if(/execSync\(\\\"rg /.test(c))hits.push(f);}}}walk(dir);if(hits.length){console.error('rg leak:\\n'+hits.join('\\n'));process.exit(1)}console.log('ok')"`

## Arquivos

- `.claude/spec/2026-05-23-dashboard-design-system/spec.md` (AC-4, AC-5)
- `.claude/spec/2026-05-23-dashboard-design-system/wave-4-ui/spec.md` (AC-W4-6, AC-W4-8, AC-W4-9)

## Tarefas

- [x] Reescrever AC-4 (parent): `styles/theme.css` check (Node fs walk).
- [x] Reescrever AC-5 (parent): `@/components/ds` import check (Node fs walk).
- [x] Reescrever AC-W4-6: imports `@/components/(specs|workspace|...)` check.
- [x] Reescrever AC-W4-8: `--color-ok`/`--color-accent-mustard` check.
- [x] Reescrever AC-W4-9: `(text|bg)-red-(400|500|600|700)` check.
- [x] Smoke-test dos 3 ACs convertidos (W4-6/8/9) — todos exit 0 sem matches reais.

## Limites

Editar APENAS as 2 specs listadas em `## Arquivos`. Não tocar código de produção (apps/dashboard, scripts/, mustard-rt source).

## Padrão do walker (template canônico para todos os ACs)

```js
node -e "const fs=require('fs');const p=require('path');const pat=/PATTERN/;const root='apps/dashboard/src';const exts=['.tsx','.ts','.jsx','.js','.mjs','.cjs','.css'];const hits=[];function walk(d){for(const e of fs.readdirSync(d,{withFileTypes:true})){if(e.name==='node_modules'||e.name==='.git'||e.name==='dist')continue;const f=p.join(d,e.name);if(e.isDirectory())walk(f);else if(exts.some(x=>e.name.endsWith(x))){if(pat.test(fs.readFileSync(f,'utf8')))hits.push(f)}}}walk(root);if(hits.length){console.error('matches:\\n'+hits.join('\\n'));process.exit(1)}console.log('ok')"
```

## Checklist

- [x] AC-4 reescrito
- [x] AC-5 reescrito
- [x] AC-W4-6 reescrito
- [x] AC-W4-8 reescrito
- [x] AC-W4-9 reescrito
- [x] `mustard-rt run qa-run --spec 2026-05-23-dashboard-design-system` roda sem `rg` no PATH
- [x] AC-TF-3 (zero `execSync\('rg ` em specs) passa
