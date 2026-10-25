//! A forma enxuta da leitura de um bloco: o conteúdo de cada item, sem as
//! marcas que o binário carimba para si.
//!
//! Cada linha sai sem a versão do formato (`v`) e sem a sessão. O número
//! (`id`) sai quando a linha tem código, que é o jeito de citar o item. Fora
//! da conversa, saem a hora e o autor; na conversa, o autor fica, e a hora
//! fica até o minuto, porque diz quem falou e quando. Toda ligação entre itens
//! sai pelo código do item apontado: os campos que o tipo declara como
//! referência (`result`, `closes`, `covers`, `contracts`, `criteria`,
//! `depends_on` da tarefa, `reply_to` da resposta e os outros) e os que
//! qualquer linha pode trazer (`origin`, `replaces`, `covered_by`), e os
//! números simples
//! que apontam itens (as tarefas e a entrega da onda, os itens e a entrega do
//! envio, o critério medido, o ponto aberto em que a fala foi dita). O número
//! sem código fica como está, e o número que não aponta item (as ondas em
//! `depends_on` da onda, as contagens, as medidas) não vira código.
//!
//! Cada fato sai sem as marcas de conferência: as impressões da fonte, a
//! classificação, a verificação e a situação da fonte. O mesmo fato (a mesma
//! fonte e o mesmo texto) sai uma vez na resposta: onde ele volta, sai
//! `{"same_as": "<código>", "fact": <posição>}`, que aponta o item em que ele
//! saiu inteiro e a posição dele ali, contada a partir de 1.
//!
//! Numa spec de campo, o mesmo ponto trazia os mesmos cinco fatos de outro,
//! cada um com duas impressões de 64 letras, e a leitura do bloco passava de
//! 40 mil letras. A leitura do item pelo código (`item-<código>`) continua com
//! o evento inteiro.

use std::collections::BTreeMap;

use mustard_core::domain::spec_events::{Block, Kind, TypeSpec, shown_line, type_spec};
use serde_json::{Map, Value, json};

/// As marcas que nenhuma linha enxuta mostra.
const STAMPS: &[&str] = &["v", "session"];

/// As ligações que qualquer linha pode trazer sem o tipo declará-las, e que
/// a linha enxuta mostra pelo código. As que o tipo declara como referência
/// vêm da declaração dele ([`points_at_item`]).
const LINKS: &[&str] = &["origin", "replaces", "covered_by"];

/// Os campos que o tipo declara como número simples, mas que apontam itens:
/// as tarefas da onda em ordem e a entrega que ela continua, os itens e a
/// entrega do envio, o critério que a medida rodou e o ponto do levantamento
/// que estava aberto quando o usuário falou. A gravação e a conferência
/// seguem com o número, que a resolução dos códigos lê como ordinal; só a
/// leitura os mostra pelo código. O `depends_on` da onda leva números de
/// onda e fica fora.
const ITEM_NUMBERS: &[(&str, &str)] = &[
    ("message", "during"),
    ("wave", "order"),
    ("wave", "summary"),
    ("send", "items"),
    ("send", "summary"),
    ("criterion_run", "criterion"),
    ("stage_run", "criterion"),
];

/// As marcas de conferência de um fato: as impressões da fonte e da medida,
/// a classificação, a verificação e a situação da fonte.
const FACT_MARKS: &[&str] =
    &["source_version", "current_source_version", "classification", "verification", "source_status", "input_sha256", "output_sha256"];

/// Quantas letras da hora ficam na conversa: `2026-10-22T14:38`.
const MINUTE: usize = 16;

/// A leitura enxuta de uma resposta: lembra cada fato já mostrado, para que
/// a repetição dele aponte para onde ele saiu.
pub(super) struct Lean<'a> {
    codes: &'a BTreeMap<u64, String>,
    facts: BTreeMap<(String, String), (String, usize)>,
}

impl<'a> Lean<'a> {
    /// A leitura enxuta com os códigos da spec, para as ligações.
    pub(super) fn new(codes: &'a BTreeMap<u64, String>) -> Self {
        Self { codes, facts: BTreeMap::new() }
    }

    /// A linha enxuta de `line`, na ordem da resposta. A linha que não é um
    /// objeto JSON sai como veio.
    pub(super) fn line(&mut self, line: &str) -> String {
        let Ok(Value::Object(mut fields)) = serde_json::from_str::<Value>(line) else {
            return line.to_string();
        };
        self.fields(&mut fields);
        shown_line(&fields)
    }

    fn fields(&mut self, fields: &mut Map<String, Value>) {
        let own = fields.get("code").and_then(Value::as_str).map(str::to_string).or_else(|| fields.get("id").map(Value::to_string));
        for stamp in STAMPS {
            fields.remove(*stamp);
        }
        if fields.contains_key("code") {
            fields.remove("id");
        }
        let spec = fields.get("type").and_then(Value::as_str).and_then(type_spec);
        let conversation = spec.is_some_and(|spec| spec.block == Block::Conversation);
        if conversation {
            if let Some(Value::String(at)) = fields.get_mut("at") {
                *at = to_the_minute(at);
            }
        } else {
            fields.remove("at");
            fields.remove("author");
        }
        link_by_code(spec, fields, self.codes);
        if let Some(Value::Array(facts)) = fields.get_mut("facts") {
            self.facts(facts, own.as_deref().unwrap_or_default());
        }
    }

    /// Os fatos de um item cujo código é `own`, sem as marcas de conferência;
    /// o fato que a resposta já mostrou vira o apontador para ele.
    fn facts(&mut self, facts: &mut [Value], own: &str) {
        for (at, fact) in facts.iter_mut().enumerate() {
            let Value::Object(marks) = fact else { continue };
            for mark in FACT_MARKS {
                marks.remove(*mark);
            }
            let key = (marks.get("source").map(Value::to_string).unwrap_or_default(), marks.get("text").map(Value::to_string).unwrap_or_default());
            match self.facts.get(&key) {
                Some((first, position)) => *fact = json!({"same_as": first, "fact": position}),
                None => {
                    self.facts.insert(key, (own.to_string(), at + 1));
                }
            }
        }
    }
}

/// Troca pelo código do item cada número de `fields`, uma linha do tipo
/// `spec`, que aponta outro item ([`points_at_item`]), com os códigos
/// `codes` da spec. É o único tradutor das ligações: a leitura enxuta e o
/// painel passam por ele.
pub(crate) fn link_by_code(spec: Option<&TypeSpec>, fields: &mut Map<String, Value>, codes: &BTreeMap<u64, String>) {
    for (name, value) in fields.iter_mut() {
        if points_at_item(spec, name) {
            *value = by_code(codes, value);
        }
    }
}

/// O número apontado, ou a lista deles, pelo código do item; o número sem
/// código e o que já é texto ficam como estão.
fn by_code(codes: &BTreeMap<u64, String>, value: &Value) -> Value {
    match value {
        Value::Array(list) => Value::Array(list.iter().map(|one| by_code(codes, one)).collect()),
        one => one.as_u64().and_then(|id| codes.get(&id)).map_or_else(|| one.clone(), |code| json!(code)),
    }
}

/// O campo `name` de uma linha do tipo `spec` aponta outro item: é uma das
/// ligações comuns, o tipo o declara como referência (o número ou o código
/// de um item) ou é um dos números simples que apontam itens
/// ([`ITEM_NUMBERS`]). O campo que só leva números de onda não aponta item,
/// ainda que outro tipo use o mesmo nome para apontar.
fn points_at_item(spec: Option<&TypeSpec>, name: &str) -> bool {
    LINKS.contains(&name)
        || spec.is_some_and(|spec| {
            ITEM_NUMBERS.contains(&(spec.name, name))
                || spec.fields.iter().any(|field| field.name == name && matches!(field.kind, Kind::Ref | Kind::Refs))
        })
}

/// A hora até o minuto, quando ela tem a forma `AAAA-MM-DDTHH:MM…`; a que
/// não tem fica como está.
fn to_the_minute(at: &str) -> String {
    let bytes = at.as_bytes();
    let shaped = bytes.len() >= MINUTE && bytes[10] == b'T' && bytes[13] == b':' && at.is_char_boundary(MINUTE);
    if shaped { at[..MINUTE].to_string() } else { at.to_string() }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use mustard_core::domain::spec_events::TYPES;

    use super::*;

    fn read_lean(codes: &BTreeMap<u64, String>, fields: Map<String, Value>) -> Value {
        let line = Lean::new(codes).line(&Value::Object(fields).to_string());
        serde_json::from_str(&line).expect("a lean line is a JSON object")
    }

    fn event(kind: &str, field: &str, value: Value) -> Map<String, Value> {
        let mut fields = Map::new();
        fields.insert("type".into(), json!(kind));
        fields.insert("id".into(), json!(9));
        fields.insert("code".into(), json!("MSTD-TASK-0009"));
        fields.insert(field.into(), value);
        fields
    }

    /// A lista dos vínculos sai da declaração de cada tipo, a mesma que a
    /// conferência usa para aceitar o número ou o código: o campo novo que
    /// aponta um item entra na lista sozinho.
    #[test]
    fn every_reference_a_type_declares_reads_by_the_item_code() {
        let codes = BTreeMap::from([(1, "MSTD-MSG-0001".to_string()), (2, "MSTD-CTX-0001".to_string())]);
        let mut seen = BTreeSet::new();
        for spec in TYPES {
            for field in spec.fields.iter().filter(|field| matches!(field.kind, Kind::Ref | Kind::Refs)) {
                let (given, shown) = match field.kind {
                    Kind::Refs => (json!([1, 2]), json!(["MSTD-MSG-0001", "MSTD-CTX-0001"])),
                    _ => (json!(1), json!("MSTD-MSG-0001")),
                };
                let line = read_lean(&codes, event(spec.name, field.name, given));
                assert_eq!(line[field.name], shown, "{} links {} by code: {line}", spec.name, field.name);
                seen.insert(field.name);
            }
        }
        for named in ["covers", "contracts", "criteria", "depends_on"] {
            assert!(seen.contains(named), "{named} is a declared reference: {seen:?}");
        }
    }

    /// O mesmo nome de campo aponta outras ondas num tipo e itens em outro:
    /// o `depends_on` da onda leva o número das ondas de que ela depende e
    /// fica número; o da tarefa aponta tarefas e sai pelo código.
    #[test]
    fn a_wave_number_that_shares_a_reference_name_stays_a_number() {
        let codes = BTreeMap::from([(1, "MSTD-MSG-0001".to_string()), (2, "MSTD-CTX-0001".to_string())]);
        let wave = read_lean(&codes, event("wave", "depends_on", json!([1, 2])));
        assert_eq!(wave["depends_on"], json!([1, 2]), "the wave keeps the waves it depends on as numbers: {wave}");
        let task = read_lean(&codes, event("task", "depends_on", json!([1, 2])));
        assert_eq!(task["depends_on"], json!(["MSTD-MSG-0001", "MSTD-CTX-0001"]));
    }

    /// Os números simples que não apontam item, campo a campo: números de
    /// onda, contagens, medidas, o processo do hospedeiro, a pendência do
    /// projeto (`P-<n>`), as lições do banco de lições e a posição até onde a
    /// cópia da página levou o registro. O campo numérico novo entra aqui ou
    /// em [`ITEM_NUMBERS`]: o teste abaixo recusa o que não está em nenhum.
    const PLAIN_NUMBERS: &[(&str, &[&str])] = &[
        ("injection", &["chars"]),
        (
            "call",
            &[
                "ms",
                "search_native_bytes",
                "search_delivered_bytes",
                "input_bytes",
                "output_bytes",
                "estimated_input_tokens",
                "estimated_output_tokens",
                "search_local_model_calls",
                "search_remote_model_calls",
                "search_accepted_choices",
                "search_scan_wait_ms",
                "search_lock_wait_ms",
                "search_vectors_ms",
                "search_investigate_ms",
                "search_jev_ms",
                "knowledge_session_model_calls",
                "knowledge_local_model_calls",
                "knowledge_remote_model_calls",
                "knowledge_input_tokens",
                "knowledge_output_tokens",
                "filter_ms",
                "tokens",
                "cost_micro_usd",
                "requests",
                "candidates",
                "returned",
            ],
        ),
        ("copy", &["last"]),
        ("rule", &["waves"]),
        ("limit", &["waves"]),
        ("contract", &["waves"]),
        ("error", &["waves"]),
        ("edge_case", &["waves"]),
        ("out_of_scope", &["waves"]),
        ("decision", &["waves"]),
        ("criterion_run", &["exit", "ms"]),
        ("stage_run", &["ms", "exit", "attempt", "waves"]),
        ("validation_failure", &["wave"]),
        ("wave", &["n", "depends_on"]),
        ("task", &["wave"]),
        (
            "send",
            &["wave", "lines", "chars", "lessons", "steps", "tokens", "caller_steps", "caller_tokens", "claude_pid", "claude_started", "reviews"],
        ),
        ("delivered", &["wave", "fixes"]),
        ("step", &["wave"]),
        ("verdict", &["wave"]),
        ("commit", &["waves", "unchanged"]),
        ("deferred", &["pending"]),
    ];

    fn is_plain_number(kind: &str, field: &str) -> bool {
        PLAIN_NUMBERS.iter().any(|(listed, fields)| *listed == kind && fields.contains(&field))
    }

    /// Todo campo que um tipo declara como número ou lista de números tem
    /// destino decidido: ou aponta um item e a leitura enxuta o mostra pelo
    /// código, ou está na lista dos números simples. Nunca os dois, e a
    /// lista dos números simples só cita campos numéricos que existem.
    #[test]
    fn every_number_a_type_declares_is_either_an_item_link_or_a_listed_plain_number() {
        let mut undecided = Vec::new();
        let mut both = Vec::new();
        for spec in TYPES {
            for field in spec.fields.iter().filter(|field| matches!(field.kind, Kind::Int | Kind::Ints)) {
                match (points_at_item(Some(spec), field.name), is_plain_number(spec.name, field.name)) {
                    (false, false) => undecided.push(format!("{}.{}", spec.name, field.name)),
                    (true, true) => both.push(format!("{}.{}", spec.name, field.name)),
                    _ => {}
                }
            }
        }
        assert!(undecided.is_empty(), "numbers neither linked by code nor listed as plain: {undecided:?}");
        assert!(both.is_empty(), "numbers both linked by code and listed as plain: {both:?}");
        for (kind, fields) in PLAIN_NUMBERS {
            for name in *fields {
                let declared = type_spec(kind)
                    .is_some_and(|spec| spec.fields.iter().any(|field| field.name == *name && matches!(field.kind, Kind::Int | Kind::Ints)));
                assert!(declared, "{kind}.{name} is listed as a plain number but the type does not declare it as one");
            }
        }
    }

    /// Os números simples que apontam itens saem pelo código: as tarefas e a
    /// entrega da onda, os itens e a entrega do envio, o critério medido e o
    /// ponto aberto em que a fala foi dita.
    #[test]
    fn the_plain_numbers_that_point_at_items_read_by_the_item_code() {
        let codes = BTreeMap::from([(1, "MSTD-TASK-0001".to_string()), (2, "MSTD-DELIV-0002".to_string())]);
        for (kind, field, given, shown) in [
            ("message", "during", json!(1), json!("MSTD-TASK-0001")),
            ("wave", "order", json!([2, 1]), json!(["MSTD-DELIV-0002", "MSTD-TASK-0001"])),
            ("wave", "summary", json!(2), json!("MSTD-DELIV-0002")),
            ("send", "items", json!([1, 2]), json!(["MSTD-TASK-0001", "MSTD-DELIV-0002"])),
            ("send", "summary", json!(2), json!("MSTD-DELIV-0002")),
            ("criterion_run", "criterion", json!(1), json!("MSTD-TASK-0001")),
            ("stage_run", "criterion", json!(1), json!("MSTD-TASK-0001")),
        ] {
            let line = read_lean(&codes, event(kind, field, given));
            assert_eq!(line[field], shown, "{kind} shows {field} by code: {line}");
        }
    }

    #[test]
    fn a_time_without_the_date_and_hour_shape_stays_as_it_is() {
        for at in ["ontem", "", "2026-10-22 14:38:48", "2026-10-22T14:3ç:48"] {
            assert_eq!(to_the_minute(at), at);
        }
    }
}
