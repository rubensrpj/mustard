'use strict';

// Catalog of Mustard env vars surfaced in the dashboard Settings tab.
// Source-of-truth: hooks under .claude/hooks read these at runtime.
// `valueDocs` explains each option in plain language so non-technical users
// can pick the right mode without reading the hooks source.

const COMMON_GATE_DOCS = {
  strict: 'Bloqueia a ação quando o gate falha. Use quando o trabalho é crítico (produção, código de outras pessoas) e você quer travar o erro antes de propagar.',
  warn: 'Permite a ação mas mostra um aviso amarelo. Use durante adoção/migração — você vê onde o gate dispara sem travar o fluxo até estar tudo limpo.',
  off: 'Desliga o gate completamente. Use em projetos pessoais ou exploratórios onde você prefere velocidade a guardrails.',
};

const ENV_CATALOG = [
  {
    group: 'Pipeline Gates',
    desc: 'Decidem o que pode bloquear transições entre fases (PLAN → EXECUTE → QA → CLOSE). São os "vetos" automáticos do Mustard antes de declarar uma spec terminada.',
    keys: [
      {
        key: 'MUSTARD_QA_GATE_MODE',
        default: 'strict',
        options: ['strict','warn','off'],
        desc: 'Antes de fechar uma spec (CLOSE), o Mustard exige que a fase QA tenha rodado todos os Acceptance Criteria com sucesso. Este parâmetro decide se uma falha de AC bloqueia, alerta ou é ignorada.',
        valueDocs: {
          strict: 'Bloqueia o CLOSE se QA não rodou ou se algum AC falhou. Recomendado para garantir que nenhuma spec termine sem ter sido validada.',
          warn: 'Permite CLOSE mas avisa. Útil quando você está adotando QA agora e ainda tem specs antigas sem AC executável.',
          off: 'QA não bloqueia. Só use se você valida acceptance fora do Mustard (ex: outro CI).',
        },
      },
      {
        key: 'MUSTARD_CLOSE_GATE_MODE',
        default: 'strict',
        options: ['strict','warn','off'],
        desc: 'O CLOSE roda build + lint + test antes de marcar uma spec como concluída. Este parâmetro decide o que acontece se algum desses falhar.',
        valueDocs: {
          strict: 'Bloqueia CLOSE se qualquer um (build/lint/test) falhar. Padrão saudável — não dá para fechar com código quebrado.',
          warn: 'Permite CLOSE mas registra um aviso. Use quando os testes ainda não cobrem tudo ou estão flaky.',
          off: 'Não verifica build/lint/test no CLOSE. Use só em sandbox.',
        },
      },
      {
        key: 'MUSTARD_CHECKLIST_GATE_MODE',
        default: 'strict',
        options: ['strict','warn','off'],
        desc: 'A spec tem uma seção "## Checklist" com itens que precisam ser marcados como `[x]` durante o EXECUTE. Este parâmetro decide o que acontece se sobraram itens em aberto na hora do CLOSE.',
        valueDocs: {
          strict: 'Bloqueia CLOSE se houver qualquer item `[ ]` na checklist. Garante que nenhum item da spec fica esquecido.',
          warn: 'Permite CLOSE com itens em aberto mas avisa. Use quando você decide cortar escopo no meio do EXECUTE.',
          off: 'Ignora a checklist no CLOSE. Útil em specs experimentais sem lista detalhada.',
        },
      },
    ],
  },
  {
    group: 'Hygiene',
    desc: 'Hooks que rodam durante Edit/Write para evitar erros comuns: código duplicado, naming fora do padrão, specs/skills crescendo demais.',
    keys: [
      {
        key: 'MUSTARD_DUPLICATION_MODE',
        default: 'warn',
        options: ['strict','warn','off'],
        desc: 'Detecta quando você está colando blocos de código quase idênticos em arquivos diferentes (≥6 linhas iguais). Indica oportunidade de extrair helper/função compartilhada.',
        valueDocs: {
          strict: 'Bloqueia o Write/Edit que cria duplicação. Forte demais para a maioria dos casos — só use em codebases onde duplicação é proibida.',
          warn: 'Mostra aviso mas deixa salvar. Você decide se vale extrair agora ou depois.',
          off: 'Não verifica duplicação. Recomendado se a heurística está dando muitos falsos positivos.',
        },
      },
      {
        key: 'MUSTARD_CONVENTION_MODE',
        default: 'warn',
        options: ['strict','warn','off'],
        desc: 'Verifica se nomes de arquivos, classes e funções batem com o padrão dominante do projeto (detectado via scan: kebab-case, PascalCase, etc).',
        valueDocs: {
          strict: 'Bloqueia o Write se o nome quebra a convenção. Use em projetos maduros onde naming é parte do estilo.',
          warn: 'Avisa mas permite. Bom default — você vê o desvio mas decide.',
          off: 'Não verifica naming. Use em projetos com convenções mistas.',
        },
      },
      {
        key: 'MUSTARD_SPEC_SIZE_MODE',
        default: 'warn',
        options: ['strict','warn','off'],
        desc: 'Specs muito longas (>500 linhas) ficam difíceis de revisar e tendem a misturar várias mudanças. Este gate avisa quando o spec.md cresce além do limite.',
        valueDocs: {
          strict: 'Bloqueia salvar spec >500 linhas. Força você a quebrar em waves.',
          warn: 'Avisa mas deixa salvar. Bom para times que ainda estão calibrando o que é "uma spec".',
          off: 'Não checa tamanho. Use quando você prefere uma spec única gigante.',
        },
      },
      {
        key: 'MUSTARD_SKILL_SIZE_MODE',
        default: 'warn',
        options: ['strict','warn','off'],
        desc: 'A Anthropic recomenda SKILL.md com body ≤200 linhas (ideal) ou ≤500 (máximo). Skills mais longas devem usar progressive disclosure (refs em arquivos separados).',
        valueDocs: {
          strict: 'Bloqueia salvar SKILL.md acima do limite. Força progressive disclosure.',
          warn: 'Avisa mas permite. Útil quando você está refatorando skills antigas.',
          off: 'Não checa tamanho de skills.',
        },
      },
      {
        key: 'MUSTARD_SKILL_VALIDATE_LINES_MODE',
        default: 'warn',
        options: ['strict','warn','off'],
        desc: 'Validação adicional de skill: frontmatter YAML correto, name único, description com gatilho útil, sem `<!-- mustard:generated -->` antes do frontmatter.',
        valueDocs: {
          strict: 'Bloqueia salvar SKILL.md mal formado. Garante que toda skill é descobrível.',
          warn: 'Avisa mas permite. Bom para iteração rápida.',
          off: 'Não valida estrutura. Risco: skills viram inúteis sem frontmatter correto.',
        },
      },
    ],
  },
  {
    group: 'Tool Use',
    desc: 'Hooks de roteamento de modelo, comandos Bash e ergonomia das ferramentas. Aqui mora a maior parte da economia de tokens.',
    keys: [
      {
        key: 'MUSTARD_BASH_REDIRECT_MODE',
        default: 'strict',
        options: ['strict','warn','off'],
        desc: 'Quando o Claude tenta rodar `grep`, `cat`, `find`, `head`, `tail` via Bash, esse hook redireciona para Grep/Read/Glob nativos. Tools nativas usam menos tokens e dão output estruturado.',
        valueDocs: {
          strict: 'Bloqueia o comando Bash redundante e força a tool nativa. Maior economia de tokens.',
          warn: 'Sugere a tool nativa mas deixa rodar Bash. Útil quando você precisa de flags específicas que a Grep não suporta.',
          off: 'Não redireciona. Use só se a tool nativa está dando resultado errado.',
        },
      },
      {
        key: 'MUSTARD_MODEL_GATE_MODE',
        default: 'strict',
        options: ['strict','warn','off'],
        desc: 'Quando uma Task é despachada com um modelo mais caro do que a tabela de roteamento prevê (ex: Sonnet → Opus em uma exploração), esse gate bloqueia. Downgrades (Opus → Sonnet) sempre passam.',
        valueDocs: {
          strict: 'Bloqueia upgrades involuntários de modelo. Evita gastar Opus em tarefas que rodam fine com Sonnet/Haiku.',
          warn: 'Permite o upgrade mas registra. Use durante diagnóstico de qualidade.',
          off: 'Roda qualquer modelo. Use só se você confia no modelo escolhido pelo agente.',
        },
      },
      {
        key: 'MUSTARD_COMMIT_GATE_MODE',
        default: 'warn',
        options: ['strict','warn','off'],
        desc: 'Antes de `git commit`, esse hook verifica se há credenciais staged (.env, *.pem, tokens) ou se o build está quebrado. Evita vazamento de secret e commits que sobem código broken para review.',
        valueDocs: {
          strict: 'Bloqueia o commit se detectar secret ou build broken. Recomendado para trabalho em time.',
          warn: 'Avisa mas deixa commitar. Bom para prototipação onde build pode estar broken intencionalmente.',
          off: 'Não verifica. Risco real de vazar credencial — use com cuidado.',
        },
      },
    ],
  },
  {
    group: 'UX & Profile',
    desc: 'Comportamentos opcionais e perfis globais. Mexa aqui se quer customizar a experiência sem mudar gates.',
    keys: [
      {
        key: 'MUSTARD_PROMPT_HINT_MODE',
        default: 'off',
        options: ['off','on'],
        desc: 'Injeta hints contextuais no prompt do usuário (ex: "considere rodar /mustard:scan antes"). Pode ser útil para onboarding mas alguns acham intrusivo.',
        valueDocs: {
          on: 'Mustard adiciona dicas curtas relacionadas ao seu prompt. Bom para descobrir comandos.',
          off: 'Sem dicas — ambiente limpo. Default recomendado para usuários experientes.',
        },
      },
      {
        key: 'MUSTARD_HOOK_PROFILE',
        default: 'standard',
        options: ['minimal','standard','strict'],
        desc: 'Perfil agregado que afeta vários hooks de uma vez. É um dial geral entre velocidade e segurança.',
        valueDocs: {
          minimal: 'Roda só o essencial (segurança crítica). Mais rápido mas menos guardrails. Para uso pessoal/exploratório.',
          standard: 'Perfil balanceado (default). Hooks importantes em modo warn, gates críticos em strict.',
          strict: 'Tudo no modo mais verificador. Mais lento mas captura mais erros antes de salvar/comitar. Para código compartilhado.',
        },
      },
      {
        key: 'MUSTARD_EPIC_COMPACT',
        default: '0',
        options: ['0','1'],
        desc: 'Em epics multi-wave, o harness log pode crescer com eventos granulares de cada wave. Se ligado, compacta esses eventos no fim do epic, mantendo só o resumo.',
        valueDocs: {
          '0': 'Mantém todos os eventos individuais. Melhor para debug detalhado.',
          '1': 'Compacta no fim do epic. Útil para reduzir tamanho do harness log em projetos grandes.',
        },
      },
    ],
  },
];

const KNOWN_KEYS = ENV_CATALOG.reduce((acc, g) => acc.concat(g.keys.map(k => k.key)), []);

function defaultsMap() {
  const m = {};
  for (const g of ENV_CATALOG) for (const k of g.keys) m[k.key] = k.default;
  return m;
}
function isKnownKey(key) { return KNOWN_KEYS.indexOf(key) >= 0; }
function isValidValue(key, value) {
  for (const g of ENV_CATALOG) {
    for (const k of g.keys) {
      if (k.key === key) return k.options.indexOf(String(value)) >= 0;
    }
  }
  return false;
}

module.exports = { ENV_CATALOG, KNOWN_KEYS, defaultsMap, isKnownKey, isValidValue };
