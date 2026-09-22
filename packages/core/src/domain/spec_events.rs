//! `spec_events` — o arquivo de eventos de uma spec (`spec.ndjson`).
//!
//! Uma spec é um arquivo só, com um evento por linha. Este módulo guarda o que
//! vale para toda linha: os 36 tipos, o envelope, os campos obrigatórios de
//! cada tipo, o bloco de cada tipo e a leitura por bloco e por passo.
//!
//! Funciona porque os tipos são fixos: o tipo mora sempre no campo `type`,
//! cada tipo vai sempre para o mesmo bloco, e o gravador recusa tipo
//! desconhecido e campo obrigatório vazio. Ler um bloco é filtrar as linhas
//! pelo tipo e, numa onda, pelo número dela.
//!
//! Nada é apagado. `remove` tira itens da leitura e deixa as linhas no arquivo,
//! com o motivo; a versão nova de um item é um evento do mesmo tipo com
//! `replaces` apontando a antiga, que some da leitura. O expurgo é a exceção,
//! e não apaga o item: troca por "…", na própria linha, só o trecho que nunca
//! podia ter sido gravado, e o item continua na leitura com o resto do texto.
//! Uma linha expurgada pelo formato antigo, que ficou só com o envelope, segue
//! fora da leitura.
//!
//! Cada item tem um código, `MSTD-<sigla>-<NNNN>`, que o binário grava na
//! linha: o maior número já dado ao tipo, mais 1. A versão nova de um item
//! grava o código da antiga. Como o código mora na linha, apagar outra linha à
//! mão não muda código nenhum, e um número que saiu não volta. Quem aponta um
//! item (`replaces`, os alvos de `remove` e `purge`) pode usar o número do
//! evento ou esse código.
//!
//! Função pura: sem disco e sem relógio. A trava, a gravação e o caminho do
//! arquivo moram em `io::spec_events`.
//!
//! O módulo é uma porta: cada assunto mora numa parte da pasta dele, e os
//! caminhos públicos continuam os mesmos.

mod against;
mod check;
mod codes;
mod line;
mod message;
mod purge;
mod read;
mod refusal;
mod search;
mod types;

pub use against::{carry_closed_identity, check_against, resolve_codes, Effects};
pub use check::{normalize, validate};
pub(crate) use check::{check_field, is_empty};
pub use codes::code_after;
pub use line::{render_line, shown_line, stamp};
pub use message::{check_message, pr_message, MessageRefusal, MESSAGE_BODY_MAX, MESSAGE_TITLE_MAX};
pub use purge::{purge_excerpts, purge_lines};
pub use read::{parse_log, Hidden, SkipReason, SkippedLine, SpecEvent, SpecLog, Step, TimeFilter};
pub use refusal::{Refusal, TaskDeclaration};
pub use search::{found_by, refresh_search_lines, search_field, search_terms};
pub use types::{
    type_names, type_spec, Block, BlockQuery, EventRef, Field, Kind, TypeSpec, DELIVERED_MAX_CHARS, METRIC_TYPES,
    PHASES, TYPES, WORK_KINDS,
};
pub(crate) use types::{opt, req};

/// A leitura da fonte que cita um arquivo mora na conferência das citações.
pub use crate::domain::citation::file_citation;

/// A versão do formato de cada linha. O leitor entende as anteriores.
pub const FORMAT_VERSION: u64 = 1;

/// Quem pode ter produzido um evento.
pub const AUTHORS: &[&str] = &["user", "assistant", "hook", "binary", "wave", "review", "skill"];

/// Quem grava pelo comando `write` sem dizer quem é: o assistente.
pub const DEFAULT_AUTHOR: &str = "assistant";

/// Os campos que só o binário escreve. O que vier neles de quem grava é
/// descartado e trocado, menos os de [`REFUSED_FIELDS`].
pub const BINARY_FIELDS: &[&str] = &["v", "id", "at", "search", "code"];

/// Os campos do binário que, vindos de quem grava, recusam o evento em vez de
/// serem descartados. Quem manda um código quer apontar um item, e trocar o
/// código em silêncio criaria um item novo no lugar: o item se aponta por
/// `replaces` ou pelos alvos de `remove` e `purge`.
pub const REFUSED_FIELDS: &[&str] = &["code"];

/// O campo que marca uma linha expurgada pelo formato antigo, que tirava o
/// texto inteiro; guarda o número do expurgo.
pub const PURGED_FIELD: &str = "purged";

/// O que fica no lugar do trecho expurgado.
pub const PURGED_MARK: &str = "…";

/// O que os testes das partes dividem — o objeto de um JSON, a conferência
/// de um rascunho e uma linha do arquivo —, os testes que as provas gravadas
/// chamam por este caminho e a medida das linhas de cada parte.
#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::{json, Map, Value};

    use super::*;
    use crate::platform::i18n::Locale;

    pub(super) fn obj(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(map) => map,
            other => panic!("not an object: {other}"),
        }
    }

    pub(super) fn checked(event_type: &str, draft: Value) -> Result<(), Refusal> {
        validate(&normalize(obj(draft), event_type))
    }

    pub(super) fn line(id: u64, event_type: &str, extra: &str) -> String {
        format!("{{\"v\":1,\"id\":{id},\"at\":\"2026-09-12T10:00:00-03:00\",\"type\":\"{event_type}\"{extra}}}\n")
    }

    /// Uma spec de teste com 8 ondas e 40 critérios abre um pull request: o
    /// corpo cabe no teto, o título cabe no teto, um título maior é recusado
    /// com a mensagem do limite, e um resumo com link da conversa, o nome do
    /// modelo, um e-mail ou um caminho da máquina é recusado apontando o
    /// trecho.
    #[test]
    fn a_mensagem_do_pull_request_cabe_nos_limites_e_recusa_dado_de_usuario() {
        let mut lines: Vec<String> = Vec::new();
        let mut id = 0u64;
        let mut push = |fields: Value| {
            id += 1;
            let mut map = obj(fields);
            map.insert("v".into(), json!(1));
            map.insert("id".into(), json!(id));
            map.insert("at".into(), json!("2026-09-16T10:00:00-03:00"));
            map.insert("author".into(), json!("assistant"));
            lines.push(render_line(&map));
            id
        };
        push(json!({"type": "context", "text": "Deixar o Mustard enxuto. E o resto da prosa."}));
        for wave in 1..=8u64 {
            push(json!({
                "type": "delivered",
                "wave": wave,
                "files": ["a.rs"],
                // Uma frase só, longa: sem ponto no meio, ela chega inteira ao
                // corpo, e as oito juntas passam do teto — que é o que faz a
                // dobra das listas em contagem ser realmente exercida aqui.
                "text": format!("A onda {wave} entregou {}", "um detalhe e ".repeat(60)),
            }));
        }
        let mut criteria: Vec<u64> = Vec::new();
        for n in 1..=40u64 {
            criteria.push(push(json!({
                "type": "criterion",
                "when": format!("o caso {n} acontece"),
                "then": "a resposta é a combinada",
                "proof": "teste",
            })));
        }
        push(json!({
            "type": "criterion_run",
            "criterion": criteria[3],
            "result": "fail",
            "exit": 1,
            "ms": 12,
        }));
        let resumo = push(json!({"type": "pr_summary", "text": "O portão lê o estado."}));

        let log = parse_log(&lines.join("\n"));
        let (title, body) = pr_message(&log).expect("a spec tem objetivo e resumo");
        assert_eq!(title, "Deixar o Mustard enxuto.");
        assert!(title.chars().count() <= MESSAGE_TITLE_MAX, "título: {}", title.chars().count());
        assert!(
            body.chars().count() <= MESSAGE_BODY_MAX,
            "corpo com {} caracteres: as listas tinham de virar contagem",
            body.chars().count(),
        );
        assert!(body.contains("O portão lê o estado."), "o resumo abre o corpo: {body}");
        assert!(body.contains("Ondas entregues: 8."), "as listas viraram contagem: {body}");
        assert!(body.contains("Critérios: 40"), "os critérios são contados: {body}");
        assert!(body.contains("1 com falha"), "e a falha é nomeada: {body}");

        // Um título maior é recusado com a mensagem do limite.
        let longo = "x".repeat(MESSAGE_TITLE_MAX + 1);
        let refusal = check_message(&longo, "corpo", MESSAGE_TITLE_MAX, MESSAGE_BODY_MAX)
            .expect_err("um título acima do teto é recusado");
        assert_eq!(refusal.reason(), "message-too-long");
        let said = refusal.message(Locale::PtBr);
        assert!(said.contains(&MESSAGE_TITLE_MAX.to_string()), "a recusa diz o limite: {said}");

        // Cada dado de usuário é recusado apontando o trecho.
        for (resumo_ruim, esperado) in [
            ("Veja https://claude.ai/code/x para o resto.", "claude.ai"),
            ("Escrito com a ajuda do Claude.", "Claude"),
            ("Dúvidas com fulano@empresa.com.br.", "fulano@empresa.com.br"),
            ("O arquivo está em /home/fulano/projetos/x.rs.", "/home/"),
        ] {
            let mut com_dado = lines.clone();
            let mut map = obj(json!({"type": "pr_summary", "text": resumo_ruim}));
            map.insert("v".into(), json!(1));
            map.insert("id".into(), json!(resumo + 1));
            map.insert("at".into(), json!("2026-09-16T11:00:00-03:00"));
            map.insert("author".into(), json!("assistant"));
            com_dado.push(render_line(&map));
            let refusal = pr_message(&parse_log(&com_dado.join("\n")))
                .expect_err(&format!("o resumo com `{esperado}` é recusado"));
            assert_eq!(refusal.reason(), "message-forbidden-text", "{esperado}");
            let said = refusal.message(Locale::PtBr);
            assert!(
                said.to_lowercase().contains(&esperado.to_lowercase()),
                "a recusa diz o que achou: {said}",
            );
            assert!(said.contains('"'), "e mostra o trecho em que achou: {said}");
        }
    }

    #[test]
    fn a_fact_without_source_is_refused_by_its_position() {
        let point = json!({
            "block": "limits", "gap": "tamanho", "from": "gap", "status": "open", "origin": 1,
            "facts": [{"text": "a", "source": "src/a.rs:1"}, {"text": "b"}]
        });
        assert_eq!(checked("point", point).unwrap_err(), Refusal::FactWithoutSource { fact: 2 });
    }

    #[test]
    fn file_citations_are_told_apart_from_other_sources() {
        assert_eq!(file_citation("src/a.rs:10"), Some(("src/a.rs".into(), 10)));
        assert_eq!(file_citation("src\\a.rs:3-9"), Some(("src/a.rs".into(), 9)));
        assert_eq!(file_citation("cargo test → 3 passed"), None);
        assert_eq!(file_citation("354"), None);
        assert_eq!(file_citation("https://example.com:8080"), None);
        assert_eq!(file_citation("README:10"), None);
    }

    /// Nenhum arquivo do núcleo dos eventos passa do teto de linhas de
    /// código: a porta e cada parte da pasta dela, pela medida única.
    #[test]
    fn no_file_of_the_spec_events_goes_over_the_code_line_cap() {
        let gate = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("domain").join("spec_events.rs");
        assert_eq!(crate::io::fs::files_over_code_line_cap(&gate), Ok(Vec::new()));
    }
}
