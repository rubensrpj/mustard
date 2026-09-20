//! As recusas: por que um evento ou uma lição não foi gravado, ou um bloco
//! não foi lido, com a razão curta e a mensagem exata nos dois idiomas.

use crate::domain::clarity::ClarityReport;
use crate::domain::survey::GapKey;
use crate::platform::i18n::{translate, Locale};

use super::{type_names, BlockQuery, EventRef, Kind};

/// Por que um evento ou uma lição não foi gravado, ou um bloco não foi lido.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    NotAnObject { detail: String },
    UnknownType { found: String },
    MissingField { event_type: String, field: String },
    InvalidValue { event_type: String, field: String, expected: Kind },
    WrongCount { event_type: String, field: String, min: usize, max: usize, count: usize },
    FactWithoutSource { fact: usize },
    CitedFileMissing { fact: usize, path: String },
    CitedLineMissing { fact: usize, path: String, line: u64, lines: u64 },
    BinaryOnlyField { field: String },
    /// Um campo que o tipo não declara. Ele entraria calado e ficaria gravado
    /// sem ninguém ver, e nunca seria lido por nada.
    UnknownField { event_type: String, field: String, accepted: String },
    UnknownTarget { target: EventRef },
    ReplacesOtherType { id: u64, found: String, event_type: String },
    FilterMatchesNothing { event_type: String, from: String, to: String },
    UnknownBlock { found: String },
    BadSpecName { spec: String },
    NoSpecFile { spec: String },
    /// Uma gravação numa spec que ninguém abriu: sem arquivo de eventos e sem
    /// estado. O arquivo nasce com a branch, no comando que abre a spec.
    SpecNotOpen { spec: String },
    /// Uma leitura sem `--spec`, e nem a variável de ambiente, nem a branch,
    /// nem a sessão apontam uma spec.
    NoCurrentSpec,
    /// Um tipo de evento da spec gravado sem dizer a spec.
    SpecRequired { event_type: String },
    /// A lição que `replaces` aponta não existe no banco de lições.
    UnknownLesson { id: u64 },
    /// A lição não diz onde nasceu.
    LessonOriginMissing,
    /// O texto da lição repete o de outra já guardada no banco, depois de
    /// igualar espaços, maiúsculas e acentos: `id` e `text` são os dela.
    LessonRepeated { id: u64, text: String },
    /// O texto da lição não passou na conferência de escrita do fim da
    /// resposta: a lição é um resumo do assistente no jeito de escrever do
    /// projeto. `report` traz os defeitos, ditos no idioma de quem lê; ele
    /// vai numa caixa para não aumentar toda recusa.
    LessonUnclear { report: Box<ClarityReport> },
    /// Uma gravação que muda a fase da spec por uma porta que não grava essa
    /// mudança: a regra única da mudança de fase recusou.
    PhaseChangeRefused { spec: String, from: String, to: String },
    /// O `run write` com o tipo `state`, ou uma gravação dele que mudaria o
    /// estado: o estado é gravado pelos comandos do fluxo e pela testemunha.
    StateByFlowOnly { spec: String },
    /// O `run write` com um tipo que só o binário grava (a execução de um
    /// critério e o veredito da revisão), ou uma gravação dele que tiraria ou
    /// reveria um desses eventos.
    BinaryOnlyType { event_type: String, spec: String },
    /// O `run write` com o autor `binary`, que fica só para as gravações de
    /// dentro do binário.
    BinaryAuthor,
    /// O `run write` com uma mensagem do usuário, ou uma gravação dele que
    /// tiraria ou reveria uma: a fala do usuário chega pelos ganchos.
    UserMessageByHook { spec: String },
    /// Uma gravação, ou uma página, numa pasta de spec do formato antigo: o
    /// `spec.md` dela é o documento, e o binário não grava nela.
    OldFormatSpec { spec: String },
    /// O pedido adiado (`deferred`) aponta uma pendência que a lista não tem.
    DeferredUnknownPending { pending: String },
    /// O pedido adiado aponta uma pendência já fechada ou descartada.
    DeferredClosedPending { pending: String },
    /// O primeiro `context` de uma spec em levantamento, o objetivo, não
    /// aponta em `origin` uma mensagem do usuário.
    GoalOriginNotUser { spec: String, origin: String },
    /// O `run write` com o tipo `work_type`, ou uma gravação dele que tiraria
    /// ou reveria o tipo de trabalho: quem o grava é o `grill`.
    WorkTypeByGrill,
    /// A passagem para o plano, ou a aprovação, de uma spec com pontos do
    /// levantamento abertos: quantos e quais, com o código, o número e a
    /// lacuna de cada um.
    SurveyOpen { spec: String, count: usize, points: String },
    /// A passagem para o plano de uma spec cujo levantamento não gravou o
    /// tipo de trabalho.
    SurveyNotStarted { spec: String },
    /// A passagem para o plano de uma spec com lacunas do tipo de trabalho
    /// ainda sem ponto; a mensagem as nomeia no idioma de quem lê.
    SurveyGapsUnrecorded { spec: String, gaps: Vec<GapKey> },
    /// Um `point` novo no mesmo bloco e com a mesma lacuna de um ponto que já
    /// está aberto: o que existe volta no lugar do segundo.
    PointAlreadyOpen { code: String, block: String },
    /// Um `point` que fecha (`closes`) o que não é um ponto aberto; a lista
    /// dos abertos vai junto.
    PointNotOpen { id: String, open: String },
    /// Um `point` que fecha outro, mas continua com a situação `open`.
    ClosingPointOpen,
    /// Um `point` marcado "não se aplica" sem o motivo.
    NotApplicableNeedsReason,
    /// Um `remove` que tiraria um ponto aberto da leitura.
    OpenPointRemoved { code: String },
    /// Um `purge` cujo trecho não aparece no item: nem o que o pedido indica,
    /// nem algum que a procura de segredo ache.
    PurgeExcerptNotFound { code: String },
    /// Um `remove` que tiraria o ponto que fecha outro cujo original já saiu:
    /// ele é o único registro do ponto.
    ClosingPointLastRecord { code: String },
    /// O pedido montado de uma onda passa do teto de linhas mesmo com o
    /// combinado reduzido a ponteiros: a onda precisa ser dividida antes de o
    /// plano ir para a aprovação, e `parts` diz o que ficou inteiro nela,
    /// cada parte com quantas linhas ocupa.
    WavePromptTooLong { wave: u64, lines: usize, max: usize, parts: String },
    /// O texto do entregou de uma onda passa do teto de caracteres: ele volta
    /// para a janela principal e precisa caber nela.
    DeliveredTooLong { chars: usize, max: usize },
    /// Um item combinado novo, gravado depois da aprovação, que não tem dono:
    /// nenhuma tarefa o cobre, ele não diz as ondas dele nem vale no projeto
    /// todo.
    OwnerMissing { event_type: String },
    /// A tarefa ou o critério que faria uma onda ainda não entregue passar do
    /// teto de tarefas ou de provas de critério: a gravação recusa antes de a
    /// onda grande nascer, com quantas tarefas e provas ela já tem e a
    /// divisão sugerida em duas ondas, cada uma com as tarefas dela. O plano
    /// usa a mesma recusa como rede, para a onda que cresceu por outro
    /// caminho.
    WaveTooBig { wave: u64, tasks: usize, proofs: usize, first: String, second: String },
    Io { detail: String },
}

impl Refusal {
    /// A razão curta, estável, para quem lê a saída por máquina.
    #[must_use]
    pub fn reason(&self) -> &'static str {
        match self {
            Self::NotAnObject { .. } => "not-an-object",
            Self::UnknownType { .. } => "unknown-type",
            Self::MissingField { .. } => "missing-field",
            Self::InvalidValue { .. } => "invalid-value",
            Self::WrongCount { .. } => "wrong-count",
            Self::FactWithoutSource { .. } => "fact-without-source",
            Self::CitedFileMissing { .. } => "cited-file-missing",
            Self::CitedLineMissing { .. } => "cited-line-missing",
            Self::BinaryOnlyField { .. } => "binary-only-field",
            Self::UnknownField { .. } => "unknown-field",
            Self::UnknownTarget { .. } => "unknown-target",
            Self::ReplacesOtherType { .. } => "replaces-other-type",
            Self::FilterMatchesNothing { .. } => "filter-matches-nothing",
            Self::UnknownBlock { .. } => "unknown-block",
            Self::BadSpecName { .. } => "bad-spec-name",
            Self::NoSpecFile { .. } => "no-spec-file",
            Self::SpecNotOpen { .. } => "spec-not-open",
            Self::NoCurrentSpec => "no-current-spec",
            Self::SpecRequired { .. } => "spec-required",
            Self::UnknownLesson { .. } => "unknown-lesson",
            Self::LessonOriginMissing => "lesson-origin-missing",
            Self::LessonRepeated { .. } => "lesson-repeated",
            Self::LessonUnclear { .. } => "lesson-unclear",
            Self::PhaseChangeRefused { .. } => "phase-change-refused",
            Self::StateByFlowOnly { .. } => "state-by-flow-only",
            Self::BinaryOnlyType { .. } => "binary-only-type",
            Self::BinaryAuthor => "binary-author",
            Self::UserMessageByHook { .. } => "user-message-by-hook",
            Self::OldFormatSpec { .. } => "old-format-spec",
            Self::DeferredUnknownPending { .. } => "deferred-unknown-pending",
            Self::DeferredClosedPending { .. } => "deferred-closed-pending",
            Self::GoalOriginNotUser { .. } => "goal-origin-not-user",
            Self::WorkTypeByGrill => "work-type-by-grill",
            Self::SurveyOpen { .. } => "survey-open",
            Self::SurveyNotStarted { .. } => "survey-not-started",
            Self::SurveyGapsUnrecorded { .. } => "survey-gaps-unrecorded",
            Self::PointAlreadyOpen { .. } => "point-already-open",
            Self::PointNotOpen { .. } => "point-not-open",
            Self::ClosingPointOpen => "closing-point-open",
            Self::NotApplicableNeedsReason => "not-applicable-needs-reason",
            Self::OpenPointRemoved { .. } => "open-point-removed",
            Self::PurgeExcerptNotFound { .. } => "purge-excerpt-not-found",
            Self::ClosingPointLastRecord { .. } => "closing-point-last-record",
            Self::WavePromptTooLong { .. } => "wave-prompt-too-long",
            Self::DeliveredTooLong { .. } => "delivered-too-long",
            Self::OwnerMissing { .. } => "owner-missing",
            Self::WaveTooBig { .. } => "wave-too-big",
            Self::Io { .. } => "io-failed",
        }
    }

    /// A mensagem exata, no idioma pedido.
    #[must_use]
    pub fn message(&self, lang: Locale) -> String {
        let fill = |key: &str, slots: &[(&str, String)]| {
            slots
                .iter()
                .fold(translate(key, lang).to_string(), |text, (slot, value)| text.replace(slot, value))
        };
        match self {
            Self::NotAnObject { detail } => {
                fill("spec_events.not_an_object", &[("{detail}", detail.clone())])
            }
            Self::UnknownType { found } => fill(
                "spec_events.unknown_type",
                &[("{type}", found.clone()), ("{types}", type_names())],
            ),
            Self::MissingField { event_type, field } => fill(
                "spec_events.missing_field",
                &[("{type}", event_type.clone()), ("{field}", field.clone())],
            ),
            Self::InvalidValue { event_type, field, expected } => fill(
                "spec_events.invalid_value",
                &[
                    ("{type}", event_type.clone()),
                    ("{field}", field.clone()),
                    ("{expected}", expected.describe(lang)),
                ],
            ),
            Self::WrongCount { event_type, field, min, max, count } => fill(
                "spec_events.wrong_count",
                &[
                    ("{type}", event_type.clone()),
                    ("{field}", field.clone()),
                    ("{min}", min.to_string()),
                    ("{max}", max.to_string()),
                    ("{count}", count.to_string()),
                ],
            ),
            Self::FactWithoutSource { fact } => {
                fill("spec_events.fact_without_source", &[("{fact}", fact.to_string())])
            }
            Self::CitedFileMissing { fact, path } => fill(
                "spec_events.cited_file_missing",
                &[("{fact}", fact.to_string()), ("{path}", path.clone())],
            ),
            Self::CitedLineMissing { fact, path, line, lines } => fill(
                "spec_events.cited_line_missing",
                &[
                    ("{fact}", fact.to_string()),
                    ("{path}", path.clone()),
                    ("{line}", line.to_string()),
                    ("{lines}", lines.to_string()),
                ],
            ),
            Self::BinaryOnlyField { field } => {
                fill("spec_events.binary_only_field", &[("{field}", field.clone())])
            }
            Self::UnknownField { event_type, field, accepted } => fill(
                "spec_events.unknown_field",
                &[
                    ("{type}", event_type.clone()),
                    ("{field}", field.clone()),
                    ("{fields}", accepted.clone()),
                ],
            ),
            Self::UnknownTarget { target: EventRef::Id(id) } => {
                fill("spec_events.unknown_target", &[("{id}", id.to_string())])
            }
            Self::UnknownTarget { target: EventRef::Code(code) } => {
                fill("spec_events.unknown_code", &[("{code}", code.clone())])
            }
            Self::ReplacesOtherType { id, found, event_type } => fill(
                "spec_events.replaces_other_type",
                &[
                    ("{id}", id.to_string()),
                    ("{found}", found.clone()),
                    ("{type}", event_type.clone()),
                ],
            ),
            Self::FilterMatchesNothing { event_type, from, to } => fill(
                "spec_events.filter_matches_nothing",
                &[("{type}", event_type.clone()), ("{from}", from.clone()), ("{to}", to.clone())],
            ),
            Self::UnknownBlock { found } => fill(
                "spec_events.unknown_block",
                &[("{block}", found.clone()), ("{blocks}", BlockQuery::accepted_names())],
            ),
            Self::BadSpecName { spec } => {
                fill("spec_events.bad_spec_name", &[("{spec}", spec.clone())])
            }
            Self::NoSpecFile { spec } => {
                fill("spec_events.no_spec_file", &[("{spec}", spec.clone())])
            }
            Self::SpecNotOpen { spec } => {
                fill("spec_events.spec_not_open", &[("{spec}", spec.clone())])
            }
            Self::NoCurrentSpec => fill("spec_events.no_current_spec", &[]),
            Self::SpecRequired { event_type } => {
                fill("spec_events.spec_required", &[("{type}", event_type.clone())])
            }
            Self::UnknownLesson { id } => fill("lessons.unknown_lesson", &[("{id}", id.to_string())]),
            Self::LessonOriginMissing => fill("lessons.origin_missing", &[]),
            Self::LessonRepeated { id, text } => fill(
                "lessons.repeated",
                &[("{id}", id.to_string()), ("{text}", opening(text, REPEATED_TEXT_CHARS))],
            ),
            Self::LessonUnclear { report } => {
                fill("lessons.unclear", &[("{defects}", report.defects(lang).join("; "))])
            }
            Self::PhaseChangeRefused { spec, from, to } => fill(
                "spec_events.phase_change_refused",
                &[("{spec}", spec.clone()), ("{from}", from.clone()), ("{to}", to.clone())],
            ),
            Self::OldFormatSpec { spec } => {
                fill("spec_events.old_format_spec", &[("{spec}", spec.clone())])
            }
            Self::StateByFlowOnly { spec } => {
                fill("spec_events.state_by_flow_only", &[("{spec}", spec.clone())])
            }
            Self::BinaryOnlyType { event_type, spec } => fill(
                "spec_events.binary_only_type",
                &[("{type}", event_type.clone()), ("{spec}", spec.clone())],
            ),
            Self::BinaryAuthor => fill("spec_events.binary_author", &[]),
            Self::UserMessageByHook { spec } => {
                fill("spec_events.user_message_by_hook", &[("{spec}", spec.clone())])
            }
            Self::DeferredUnknownPending { pending } => {
                fill("spec_events.deferred_unknown_pending", &[("{pending}", pending.clone())])
            }
            Self::DeferredClosedPending { pending } => {
                fill("spec_events.deferred_closed_pending", &[("{pending}", pending.clone())])
            }
            Self::GoalOriginNotUser { spec, origin } => fill(
                "spec_events.goal_origin_not_user",
                &[("{spec}", spec.clone()), ("{origin}", origin.clone())],
            ),
            Self::WorkTypeByGrill => fill("grill.work_type_by_grill", &[]),
            Self::SurveyOpen { spec, count, points } => fill(
                "spec_events.survey_open",
                &[("{spec}", spec.clone()), ("{count}", count.to_string()), ("{points}", points.clone())],
            ),
            Self::SurveyNotStarted { spec } => fill("spec_events.survey_not_started", &[("{spec}", spec.clone())]),
            Self::SurveyGapsUnrecorded { spec, gaps } => fill(
                "spec_events.survey_gaps_unrecorded",
                &[
                    ("{spec}", spec.clone()),
                    ("{count}", gaps.len().to_string()),
                    ("{gaps}", gaps.iter().map(|gap| gap.label(lang)).collect::<Vec<_>>().join("; ")),
                ],
            ),
            Self::PointAlreadyOpen { code, block } => fill(
                "spec_events.point_already_open",
                &[("{code}", code.clone()), ("{block}", block.clone())],
            ),
            Self::PointNotOpen { id, open } => {
                fill("spec_events.point_not_open", &[("{id}", id.clone()), ("{open}", open.clone())])
            }
            Self::ClosingPointOpen => fill("spec_events.closing_point_open", &[]),
            Self::NotApplicableNeedsReason => fill("spec_events.not_applicable_reason", &[]),
            Self::OpenPointRemoved { code } => fill("spec_events.open_point_removed", &[("{code}", code.clone())]),
            Self::PurgeExcerptNotFound { code } => {
                fill("spec_events.purge_excerpt_not_found", &[("{code}", code.clone())])
            }
            Self::ClosingPointLastRecord { code } => {
                fill("spec_events.closing_point_last_record", &[("{code}", code.clone())])
            }
            Self::WavePromptTooLong { wave, lines, max, parts } => fill(
                "spec_events.wave_prompt_too_long",
                &[
                    ("{wave}", wave.to_string()),
                    ("{lines}", lines.to_string()),
                    ("{max}", max.to_string()),
                    ("{parts}", parts.clone()),
                ],
            ),
            Self::DeliveredTooLong { chars, max } => fill(
                "spec_events.delivered_too_long",
                &[("{chars}", chars.to_string()), ("{max}", max.to_string())],
            ),
            Self::OwnerMissing { event_type } => {
                fill("plan.owner_missing", &[("{type}", event_type.clone())])
            }
            Self::WaveTooBig { wave, tasks, proofs, first, second } => fill(
                "plan.wave_too_big",
                &[
                    ("{wave}", wave.to_string()),
                    ("{tasks}", tasks.to_string()),
                    ("{proofs}", proofs.to_string()),
                    ("{cap}", crate::domain::wave_prompt::WAVE_TASKS_CAP.to_string()),
                    ("{first}", first.clone()),
                    ("{second}", second.clone()),
                ],
            ),
            Self::Io { detail } => fill("spec_events.io_failed", &[("{detail}", detail.clone())]),
        }
    }
}

/// Quantos caracteres do texto da lição que já existe a recusa de repetição
/// mostra: o bastante para achar a lição, pouco para não copiá-la inteira.
const REPEATED_TEXT_CHARS: usize = 160;

/// O começo de `text`, com no máximo `max` caracteres; o corte ganha "…".
fn opening(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut cut: String = text.chars().take(max).collect();
    cut.push('…');
    cut
}
