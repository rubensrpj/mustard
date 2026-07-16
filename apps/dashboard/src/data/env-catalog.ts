// SPEC LANG: pt-allowed — catalog entries are pt-BR end-user copy; migration to i18n dict deferred to sub-spec 2026-05-26-dashboard-data-catalogs-i18n.
export interface EnvKey {
  /** Environment variable name — the technical identifier persisted to disk. */
  key: string;
  /**
   * Human-readable title shown as the PRIMARY label in Settings. Always
   * distinct from `key`: it describes what the knob does in plain language,
   * so a non-expert can scan the form without decoding MUSTARD_* names.
   */
  label: string;
  default: string;
  options: string[];
  desc: string;
  valueDocs: Record<string, string>;
  /**
   * Low-level knob — collapsed under an "Avançado" group in Settings. Use for
   * transport/protocol/port details most users never touch.
   */
  advanced?: boolean;
}

export interface EnvGroup {
  group: string;
  desc: string;
  keys: EnvKey[];
}

const GATE_DOCS = {
  strict: 'Bloqueia a fase se o check falhar.',
  warn: 'Loga aviso mas permite a fase continuar.',
  off: 'Desabilita o check completamente.',
};

export const ENV_CATALOG: EnvGroup[] = [
  {
    group: 'Pipeline Gates',
    desc: 'Controlam quando a pipeline bloqueia ou permite cada fase.',
    keys: [
      {
        key: 'MUSTARD_QA_GATE_MODE',
        label: 'Gate de QA antes do CLOSE',
        default: 'strict',
        options: ['strict', 'warn', 'off'],
        desc: 'Comportamento do gate de QA antes de CLOSE.',
        valueDocs: GATE_DOCS,
      },
      {
        key: 'MUSTARD_CLOSE_GATE_MODE',
        label: 'Gate de CLOSE (build + tipos + lint + testes)',
        default: 'strict',
        options: ['strict', 'warn', 'off'],
        desc: 'Comportamento do gate de CLOSE (build + type + lint + test).',
        valueDocs: GATE_DOCS,
      },
      {
        key: 'MUSTARD_COMMIT_GATE_MODE',
        label: 'Gate de commit (pre-commit)',
        default: 'warn',
        options: ['strict', 'warn', 'off'],
        desc: 'Comportamento do gate de commit (pre-commit hook).',
        valueDocs: GATE_DOCS,
      },
      {
        key: 'MUSTARD_SPEC_SIZE_MODE',
        label: 'Limite de tamanho da spec',
        default: 'warn',
        options: ['strict', 'warn', 'off'],
        desc: 'Bloqueia ou avisa quando spec excede tamanho máximo.',
        valueDocs: GATE_DOCS,
      },
      {
        key: 'MUSTARD_DEBT_GATE_MODE',
        label: 'Bloqueio de débito técnico (TODO/FIXME) no CLOSE',
        default: 'strict',
        options: ['strict', 'warn', 'off'],
        desc: 'Bloqueia CLOSE quando spec contém debt markers (future hook, TODO, FIXME, not part of wave) nas seções Tasks/Checklist/Acceptance.',
        valueDocs: {
          strict: 'Bloqueia CLOSE se debt marker em seção actionable.',
          warn: 'Loga aviso mas permite CLOSE.',
          off: 'Desabilita o check de debt markers.',
        },
      },
      {
        key: 'MUSTARD_AC_QUALITY_MODE',
        label: 'Auditoria de qualidade dos Acceptance Criteria',
        default: 'warn',
        options: ['warn', 'off'],
        desc: 'Audita Acceptance Criteria por qualidade: AC só com build/test, AC não-binário ("já validado"), path moveable (.claude/spec/active/), ou sem Command.',
        valueDocs: {
          warn: 'Loga aviso no stderr quando AC fraco detectado (default).',
          off: 'Desabilita o audit de qualidade de AC.',
        },
      },
      {
        key: 'MUSTARD_BOUNDARY_MODE',
        label: 'Detecção de expansão de escopo',
        default: 'warn',
        options: ['warn', 'strict', 'off'],
        desc: 'Compara arquivo editado com ## Files / ## Boundaries do spec ativo. Sinaliza scope expansion (ex: agent editou arquivo cascateado fora do escopo declarado).',
        valueDocs: {
          warn: 'Loga aviso no stderr + emite boundary.expansion event (default).',
          strict: 'Nega edição via permissionDecision se fora do scope.',
          off: 'Desabilita o check de boundary.',
        },
      },
      {
        key: 'MUSTARD_APPROVAL_MODE',
        label: 'Gate de aprovação do plano (PLAN) pelo usuário',
        default: 'strict',
        options: ['strict', 'warn', 'off'],
        desc: 'Exige que a aprovação de um plano Full venha do usuário: approve-spec só emite o sinal draft→approved quando existe o marcador .approved-by-user, gravado pelo observer quando você aprova o plano no plan mode (ExitPlanMode) — ou, como fallback, quando responde à pergunta de aprovação (AskUserQuestion). Impede o orquestrador de se auto-aprovar.',
        valueDocs: {
          strict: 'Bloqueia approve-spec (exit≠0) sem o marcador de aprovação do usuário.',
          warn: 'Avisa mas deixa approve-spec seguir sem o marcador.',
          off: 'Desabilita a exigência (comportamento anterior ao T5).',
        },
      },
    ],
  },
  {
    group: 'Telemetria OTEL',
    desc: 'Captura nativa de telemetria do Claude Code (cost USD real + tokens + sessions) via OTLP local. Wave 3 da spec honest-prompt-economy.',
    keys: [
      {
        key: 'CLAUDE_CODE_ENABLE_TELEMETRY',
        label: 'Ligar telemetria nativa do Claude Code',
        default: '1',
        options: ['1', '0'],
        desc: 'Liga a telemetria nativa do Claude Code CLI (necessário para o collector receber dados).',
        valueDocs: {
          '1': 'Telemetria ligada — Claude Code emite OTLP nos endpoints abaixo.',
          '0': 'Telemetria desligada — dashboard de economia ficará vermelho (sem dados).',
        },
      },
      {
        key: 'MUSTARD_HARNESS_DUAL_EMIT',
        label: 'Entrega de telemetria via collector OTEL local',
        default: '1',
        options: ['1', '0'],
        desc: 'Marca que o caminho de entrega de telemetria via collector OTEL local está ativo: o collector anexa as métricas de token/custo (pipeline.telemetry.metric) ao log NDJSON por sessão/spec. Hoje só é lido pelo diagnose-otel — quando =1 e o collector está saudável, as variáveis OTEL_* de exporter viram opcionais (env.ok=true mesmo sem elas, pois o dado já flui pelo collector).',
        valueDocs: {
          '1': 'Caminho do collector ativo — OTEL_* de exporter tratados como opcionais no diagnose-otel.',
          '0': 'Diagnose-otel exige as OTEL_* de exporter setadas para considerar env.ok.',
        },
      },
      {
        key: 'MUSTARD_DISABLE_OTEL_COLLECTOR',
        label: 'Desligar o spawn automático do coletor',
        default: '',
        options: ['', '1'],
        desc: 'Opt-out do spawn automático do collector em SessionStart. Útil em CI ou ambientes sem Bun.',
        valueDocs: {
          '': 'Collector spawnado automaticamente em SessionStart (default).',
          '1': 'Collector NÃO spawnado — você precisa rodar manualmente OU dashboard ficará vermelho.',
        },
      },
      {
        key: 'MUSTARD_OTEL_PORT',
        label: 'Porta do coletor OTEL local',
        default: '4318',
        options: [],
        desc: 'Porta loopback onde o otel-collector.js escuta (HTTP). Mude se 4318 estiver em uso por outro collector.',
        valueDocs: {},
        advanced: true,
      },
      {
        key: 'OTEL_METRICS_EXPORTER',
        label: 'Exporter de métricas',
        default: 'otlp',
        options: ['otlp', 'none'],
        desc: 'Exporter usado pelo Claude Code para enviar métricas. otlp aponta para o collector local.',
        valueDocs: {
          otlp: 'Envia para OTEL_EXPORTER_OTLP_ENDPOINT (default).',
          none: 'Não exporta métricas — dashboard cost ficará zero.',
        },
        advanced: true,
      },
      {
        key: 'OTEL_LOGS_EXPORTER',
        label: 'Exporter de logs',
        default: 'otlp',
        options: ['otlp', 'none'],
        desc: 'Exporter usado pelo Claude Code para logs (paralelo ao de métricas).',
        valueDocs: {
          otlp: 'Envia para o mesmo endpoint OTLP (default).',
          none: 'Sem export de logs.',
        },
        advanced: true,
      },
      {
        key: 'OTEL_EXPORTER_OTLP_PROTOCOL',
        label: 'Protocolo de transporte OTLP',
        default: 'http/json',
        options: ['http/json', 'http/protobuf', 'grpc'],
        desc: 'Protocolo de transporte OTLP. http/json é o testado e validado (2.1.142). http/protobuf tem bug #50567 (silent no-op em 2.1.113).',
        valueDocs: {
          'http/json': 'JSON sobre HTTP — testado, recomendado.',
          'http/protobuf': 'Protobuf sobre HTTP — pode silenciar dados em versões antigas.',
          grpc: 'gRPC — não testado nesta integração.',
        },
        advanced: true,
      },
      {
        key: 'OTEL_EXPORTER_OTLP_ENDPOINT',
        label: 'Endpoint de destino do OTLP',
        default: 'http://127.0.0.1:4318',
        options: [],
        desc: 'Endpoint de destino do OTLP. Mantenha 127.0.0.1 (loopback). Sincronize a porta com MUSTARD_OTEL_PORT.',
        valueDocs: {},
        advanced: true,
      },
      {
        key: 'OTEL_METRICS_INCLUDE_SESSION_ID',
        label: 'Incluir session_id nas métricas',
        default: 'true',
        options: ['true', 'false'],
        desc: 'Inclui session_id como atributo nas métricas (necessário para agregação by_session no dashboard).',
        valueDocs: {
          true: 'session_id presente em cada datapoint (recomendado).',
          false: 'Sem session_id — quebra a agregação por sessão do dashboard.',
        },
        advanced: true,
      },
    ],
  },
  {
    group: 'Resume Behavior',
    desc: 'Controla o quanto o /mustard:resume pergunta ao usuário antes de continuar. Tier 1 follow-up da spec honest-prompt-economy.',
    keys: [
      {
        key: 'MUSTARD_RESUME_MODE',
        label: 'Modo de retomada do pipeline',
        default: 'auto',
        options: ['auto', 'continued', 'reanalyzed', 'ask'],
        desc: 'Força um modo de resume. Default auto: continued se state <10min OR currentWave>1 sem falha; senão pergunta.',
        valueDocs: {
          auto: 'Decide pelo estado do pipeline (default).',
          continued: 'Sempre confia no pipeline-state — pula a re-análise (scan) + diff-context.',
          reanalyzed: 'Sempre re-roda a re-análise (scan) + diff-context (modo seguro).',
          ask: 'Restaura o prompt legacy a cada resume.',
        },
      },
      {
        key: 'MUSTARD_RESUME_CONFIRM',
        label: 'Quando confirmar antes de retomar',
        default: 'fresh-only',
        options: ['fresh-only', 'always', 'never'],
        desc: 'Quando perguntar "Continue from next action, or review spec first?" no Step 1.7.',
        valueDocs: {
          'fresh-only': 'Pergunta apenas em scope=full + currentWave=1 + sem completedWaves.',
          always: 'Pergunta a cada resume (comportamento antigo).',
          never: 'Nunca pergunta — auto-continue sempre.',
        },
      },
      {
        key: 'MUSTARD_QA_SHELL',
        label: 'Shell usado para rodar os Acceptance Criteria',
        default: '',
        options: [],
        desc: 'Override do shell usado pelo qa-run.js. Default no Windows tenta Git Bash; em outros sistemas usa /bin/sh. Use "cmd" para forçar cmd.exe ou caminho absoluto para bash custom.',
        valueDocs: {},
      },
    ],
  },
  {
    group: 'Cost Hooks',
    desc: 'Hooks de redução de custo: bash redirect, model routing, disable seletivo.',
    keys: [
      {
        key: 'MUSTARD_BASH_REDIRECT_MODE',
        label: 'Redirecionar comandos Bash via RTK',
        default: 'strict',
        options: ['strict', 'warn', 'off'],
        desc: 'Redireciona comandos Bash caros via RTK para economizar tokens.',
        valueDocs: {
          strict: 'Força redirecionamento; bloqueia se RTK não disponível.',
          warn: 'Tenta redirecionamento; loga aviso se falhar.',
          off: 'Desabilita o hook de redirect — comandos passam sem filtro.',
        },
      },
      {
        key: 'MUSTARD_MODEL_GATE_MODE',
        label: 'Roteamento de modelo (Haiku/Sonnet/Opus)',
        default: 'strict',
        options: ['strict', 'warn', 'off'],
        desc: 'Controla se o routing de modelo (Haiku→Sonnet→Opus) é obrigatório.',
        valueDocs: {
          strict: 'Bloqueia uso de modelo inadequado para o tipo de tarefa.',
          warn: 'Loga aviso mas permite qualquer modelo.',
          off: 'Sem restrição de modelo — Claude escolhe livremente.',
        },
      },
      {
        key: 'MUSTARD_DISABLED_HOOKS',
        label: 'Hooks desabilitados (lista CSV)',
        default: '',
        options: [],
        desc: 'CSV de nomes de hooks desabilitados (ex: rtk-rewrite,duplication-check).',
        valueDocs: {},
        advanced: true,
      },
    ],
  },
  {
    group: 'Anti-Slope',
    desc: 'Checks de duplicação e convenção para manter qualidade do código.',
    keys: [
      {
        key: 'MUSTARD_DUPLICATION_MODE',
        label: 'Detecção de código duplicado',
        default: 'off',
        options: ['strict', 'warn', 'off'],
        desc: 'Detecta e bloqueia (strict) ou avisa (warn) sobre código duplicado.',
        valueDocs: {
          strict: 'Bloqueia commit/CLOSE se duplicação detectada.',
          warn: 'Loga aviso de duplicação mas não bloqueia.',
          off: 'Desabilita o check de duplicação.',
        },
      },
      {
        key: 'MUSTARD_CONVENTION_MODE',
        label: 'Verificação de convenções de código',
        default: 'off',
        options: ['strict', 'warn', 'off'],
        desc: 'Verifica se convenções de naming/estrutura do projeto são seguidas.',
        valueDocs: {
          strict: 'Bloqueia se convenção violada.',
          warn: 'Loga aviso de convenção mas não bloqueia.',
          off: 'Desabilita o check de convenção.',
        },
      },
    ],
  },
  {
    group: 'Skill Tracking',
    desc: 'Auditoria de skills invocados via /mustard:skill ou Skill tool. Wave 4 da spec honest-prompt-economy.',
    keys: [
      {
        key: 'MUSTARD_SKILL_ORPHAN_DAYS',
        label: 'Janela de skills órfãos (dias sem uso)',
        default: '30',
        options: ['7', '14', '30', '60', '90'],
        desc: 'Janela de inatividade (dias) usada por skill-orphan-audit.js para listar skills não-invocados.',
        valueDocs: {},
      },
    ],
  },
  {
    group: 'Cluster Discovery',
    desc: 'Parâmetros de tuning do algoritmo de descoberta de clusters no /scan.',
    keys: [
      {
        key: 'MUSTARD_CLUSTER_MIN_FILES',
        label: 'Mínimo de arquivos por cluster',
        default: '5',
        options: ['2', '3', '4', '5', '6', '7', '8', '9', '10'],
        desc: 'Mínimo de arquivos por sufixo para formar um cluster.',
        valueDocs: {},
        advanced: true,
      },
      {
        key: 'MUSTARD_CLUSTER_MIN_SUFFIX_LEN',
        label: 'Tamanho mínimo do sufixo de cluster',
        default: '6',
        options: ['2', '3', '4', '5', '6', '7', '8', '9', '10'],
        desc: 'Comprimento mínimo do sufixo compartilhado para cluster.',
        valueDocs: {},
        advanced: true,
      },
      {
        key: 'MUSTARD_CLUSTER_MIN_BASE_INHERITORS',
        label: 'Mínimo de herdeiros para base-class-cluster',
        default: '3',
        options: ['2', '3', '4', '5', '6', '7', '8', '9', '10'],
        desc: 'Mínimo de herdeiros para base-class-cluster.',
        valueDocs: {},
        advanced: true,
      },
      {
        key: 'MUSTARD_CLUSTER_MAX',
        label: 'Máximo de clusters por subprojeto',
        default: '30',
        options: ['10', '30', '50', '100'],
        desc: 'Número máximo de clusters por subprojeto (excedentes vão ao stderr).',
        valueDocs: {},
        advanced: true,
      },
      {
        key: 'MUSTARD_DECORATOR_MIN',
        label: 'Mínimo de arquivos para decorator-cluster',
        default: '3',
        options: ['2', '3', '4', '5', '6', '7', '8', '9', '10'],
        desc: 'Mínimo de arquivos para decorator-cluster.',
        valueDocs: {},
        advanced: true,
      },
      {
        key: 'MUSTARD_FN_PREFIX_MIN',
        label: 'Mínimo de funções para function-prefix-cluster',
        default: '5',
        options: ['2', '3', '4', '5', '6', '7', '8', '9', '10'],
        desc: 'Mínimo de funções com mesmo prefixo para function-prefix-cluster.',
        valueDocs: {},
        advanced: true,
      },
      {
        key: 'MUSTARD_FN_PREFIX_MIN_LEN',
        label: 'Tamanho mínimo do prefixo de função',
        default: '2',
        options: ['2', '3', '4', '5'],
        desc: 'Comprimento mínimo do prefixo de função para cluster.',
        valueDocs: {},
        advanced: true,
      },
      {
        key: 'MUSTARD_NAMING_DOMINANCE',
        label: 'Share mínimo para naming dominante',
        default: '0.6',
        options: ['0.5', '0.6', '0.7', '0.8', '0.9', '0.95'],
        desc: 'Share mínimo de um padrão de naming para ser considerado dominante.',
        valueDocs: {},
        advanced: true,
      },
      {
        key: 'MUSTARD_CLUSTER_CACHE',
        label: 'Cache de clusters entre /scan',
        default: 'on',
        options: ['on', 'off'],
        desc: 'Habilita cache de clusters em .cluster-cache.json por subprojeto.',
        valueDocs: {
          on: 'Cache ativo — /scan reutiliza clusters da run anterior.',
          off: 'Sem cache — clusters recalculados a cada /scan.',
        },
        advanced: true,
      },
    ],
  },
  {
    group: 'Scan',
    desc: 'Configurações de ignorar pastas durante o /scan.',
    keys: [
      {
        key: 'MUSTARD_SCAN_IGNORE',
        label: 'Pastas ignoradas pelo /scan (lista CSV)',
        default: '',
        options: [],
        desc: 'CSV de nomes de pastas ignoradas pelo /scan (ex: Pods,vendor,assets).',
        valueDocs: {},
      },
    ],
  },
  {
    group: 'Lang',
    desc: 'Idioma padrão de specs e outputs da pipeline.',
    keys: [
      {
        key: 'MUSTARD_SPEC_LANG',
        label: 'Idioma das specs e da pipeline (BCP-47)',
        default: 'en-US',
        options: ['en-US', 'pt-BR'],
        desc: 'Idioma usado em specs geradas e headers da pipeline (BCP-47).',
        valueDocs: {
          'en-US': 'Specs e headers em inglês (padrão).',
          'pt-BR': 'Specs e headers em português.',
        },
      },
    ],
  },
];
