//! `search_door` — a porta única da busca do mapa: o caminho que toda busca
//! do Claude faz depois da triagem, seja o pedido por assunto (`run map
//! search`) ou a busca por palavra que o gancho responde (`Grep` e
//! `grep`, `rg` e `git grep` do terminal).
//!
//! O caminho tem quatro passos, e a porta guarda todos:
//!
//! 1. a triagem crava: a resposta sai dela, com a primeira peça inteira, sem
//!    filtro, sem aviso de chave e sem chamada gravada ([`pinned`]);
//! 2. não cravou: os candidatos do banco vão ao filtro num pedido só, com a
//!    frase de quem procura, a descrição que o agente deu à busca e a última
//!    fala dele ([`classify`]);
//! 3. o filtro diz não achei, e nada volta; ou diz que algum candidato serve, e
//!    voltam as peças que passam do corte, no máximo duas por padrão;
//! 4. sem filtro (desligado, sem chave), sem candidato ou com o filtro
//!    falhando, quem chamou responde só com a triagem, e o aviso do motivo
//!    sai uma vez por sessão ([`chosen_filter`], [`failure_warning`]).
//!
//! A porta devolve tipos ([`Outcome`], [`Piece`]); quem chamou monta o texto:
//! o JSON da busca por assunto, em `commands::map`, ou a resposta por função
//! da busca por palavra, em `word_search`. A montagem do filtro ([`Assemble`],
//! [`jev`]) e o aviso de uma vez por sessão ([`first_warning`]) moram aqui,
//! para os dois caminhos os usarem sem um depender do outro.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use mustard_core::domain::map_filter::{
    CutRule, FilterCandidate, FilterError, FilterRequest, MapFilter, Verdict, CUT_SHARE, EXISTS_FROM, MAX_KEPT,
};
use mustard_core::domain::map_select::{capped, select, Source, MAX_RETURNED};
use mustard_core::domain::normalize::Languages;
use mustard_core::domain::project_map::{self, MapRefusal};
use mustard_core::domain::search::CANDIDATES;
use mustard_core::io::map_search;
use mustard_core::domain::triage::Mark;
use mustard_core::io::map_triage::Triaged;
use mustard_core::platform::i18n::{translate, Locale};
use mustard_core::{FilterSetting, ProjectConfig, Setting};
use serde_json::{json, Map, Value};

use crate::shared::code_route::in_search;
use crate::shared::config_key::{NameFilter, Walk};
use crate::shared::triage_view;

/// Os números da busca com filtro, com o padrão no lugar do ausente e do
/// inválido.
pub(crate) struct Numbers {
    /// Quantos candidatos do banco vão ao filtro.
    pub(crate) candidates: usize,
    /// A parte da maior chance que passa do corte, em pontos percentuais.
    pub(crate) cut_share: usize,
    /// A chance de algum candidato servir, em pontos percentuais, a partir da
    /// qual há resposta; abaixo dela é não achei.
    pub(crate) exists_from: usize,
    /// Quantos candidatos passam do corte, no máximo.
    pub(crate) max_kept: usize,
    /// O teto de peças na resposta.
    pub(crate) max_returned: usize,
}

impl Numbers {
    /// Os números que a seção `search` da configuração diz. O valor inválido
    /// cai no padrão e deixa em `warnings` o aviso que diz a chave e o padrão,
    /// uma vez por sessão.
    pub(crate) fn read(
        root: &Path,
        session: Option<&str>,
        lang: Locale,
        config: &ProjectConfig,
        warnings: &mut Vec<String>,
    ) -> Self {
        let mut number = |key: &str, setting: Setting, default: usize| {
            if setting == Setting::Invalid && first_warning(root, session, &format!("search.{key}")) {
                warnings.push(
                    translate("map.search.bad_number", lang)
                        .replace("{key}", key)
                        .replace("{default}", &default.to_string()),
                );
            }
            setting.or(default)
        };
        Self {
            candidates: number("candidates", config.search_candidates(), CANDIDATES),
            cut_share: number("cut_share", config.search_cut_share(), (CUT_SHARE * 100.0).round() as usize),
            exists_from: number("exists_from", config.search_exists_from(), (EXISTS_FROM * 100.0).round() as usize),
            max_kept: number("max_kept", config.search_max_kept(), MAX_KEPT),
            max_returned: number("max_returned", config.search_max_returned(), MAX_RETURNED),
        }
    }
}

/// O que a porta leva de uma busca: o projeto, as palavras, a frase, o que o
/// agente disse da busca, o idioma do texto, as línguas das palavras, os
/// números, a triagem e onde a busca procura: as pastas, os filtros de nome de
/// arquivo e o jeito de ler as chaves deles.
pub(crate) struct Ask<'a> {
    pub(crate) root: &'a Path,
    pub(crate) query: &'a str,
    pub(crate) intent: &'a str,
    /// A descrição que o agente deu à busca; vazia quando ele não deu.
    pub(crate) described: &'a str,
    /// A última fala do agente antes da busca; vazia quando não há.
    pub(crate) said: &'a str,
    pub(crate) lang: Locale,
    pub(crate) languages: &'a Languages,
    pub(crate) numbers: &'a Numbers,
    /// A triagem da pergunta: o grau, os arquivos do banco e a busca funda.
    pub(crate) triaged: &'a Triaged,
    /// As pastas buscadas, relativas à raiz; vazia é o projeto inteiro. Só os
    /// arquivos de dentro delas vão ao filtro e voltam na resposta.
    pub(crate) rels: &'a [String],
    /// Os filtros de nome de arquivo da busca (`--include`, o glob e o tipo da
    /// ferramenta); vazio não tira nenhum.
    pub(crate) filters: &'a [NameFilter],
    pub(crate) walk: Walk,
}

/// A frase que o filtro lê: a de `--intent`; sem ela, as palavras da
/// `--query`, e a palavra só vira o pedido de um pedaço de nome, no idioma do
/// texto do projeto.
pub(crate) fn phrase_of(words: &[String], query: &str, intent: &str, lang: Locale) -> String {
    match (intent.is_empty(), words) {
        (false, _) => intent.to_string(),
        (true, [word]) => translate("map.search.name_piece", lang).replace("{word}", word),
        (true, _) => query.to_string(),
    }
}

/// Uma peça da resposta: uma declaração que voltou. Nunca leva o corpo.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Piece {
    pub(crate) path: String,
    pub(crate) line: u32,
    pub(crate) end_line: u32,
    pub(crate) kind: String,
    pub(crate) name: String,
    pub(crate) signature: String,
    /// A primeira frase da documentação.
    pub(crate) doc: String,
    /// A chance que o filtro deu a ela; só as peças do corte a trazem, e não
    /// as que o corte puxou pelas ligações.
    pub(crate) score: Option<f64>,
}

impl Piece {
    /// A peça como a busca por assunto a devolve: o caminho, a linha, o fim, o
    /// tipo, o nome, a assinatura e a documentação, quando há, e a chance.
    pub(crate) fn to_value(&self) -> Value {
        let mut piece = json!({
            "path": self.path, "line": self.line, "end_line": self.end_line, "kind": self.kind, "name": self.name
        });
        if !self.signature.is_empty() {
            piece["signature"] = json!(self.signature);
        }
        if !self.doc.is_empty() {
            piece["doc"] = json!(self.doc);
        }
        if let Some(score) = self.score {
            piece["score"] = json!((score * 100.0).round() / 100.0);
        }
        piece
    }
}

/// O que o filtro devolveu à busca.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Outcome {
    /// O banco não trouxe candidato: nada a classificar, e o filtro nem foi
    /// chamado.
    NoCandidates,
    /// O filtro respondeu: o veredito e as peças, na ordem da chance. No não
    /// achei não há peça nenhuma.
    Classified { verdict: Verdict, pieces: Vec<Piece> },
    /// O filtro falhou: quem chamou responde só com a triagem.
    Failed(FilterError),
}

/// O resultado de [`classify`] e os campos da medida da chamada, para a spec
/// gravá-la.
pub(crate) struct Classified {
    pub(crate) outcome: Outcome,
    pub(crate) measured: Map<String, Value>,
}

/// A peça cravada: a primeira declaração do primeiro achado da triagem, sem
/// filtro. Só há quando a marca é cravado e o arquivo achado tem declaração
/// na lista do banco; senão, `None`, e a busca segue para o filtro.
pub(crate) fn pinned(
    root: &Path,
    (query, intent): (&str, &str),
    languages: &Languages,
    config: &ProjectConfig,
    triaged: &Triaged,
) -> Result<Option<FilterCandidate>, MapRefusal> {
    if triaged.mark() != Mark::Pinned {
        return Ok(None);
    }
    let limit = config.search_candidates().or(CANDIDATES);
    triage_view::pinned_piece(root, (query, intent), languages, limit, triaged)
}

/// O filtro da busca montado com a chave do projeto: o nome dele, que a
/// resposta e o registro da chamada dizem, e o aviso da chave, quando a do
/// ambiente vale mas o git guarda o `mustard.json` que também traz uma.
pub(crate) struct Assembled {
    pub(crate) name: &'static str,
    pub(crate) filter: Box<dyn MapFilter>,
    pub(crate) warning: Option<FilterError>,
}

/// A montagem do filtro para o projeto na raiz, com o `mustard.json` que a
/// busca leu: com a chave, o filtro; sem ela, o motivo, que vira aviso. Só a
/// busca por assunto e a busca por palavra a chamam, e só quando a
/// configuração não desliga o filtro.
pub(crate) type Assemble<'a> = dyn Fn(&Path, &ProjectConfig) -> Result<Assembled, FilterError> + 'a;

/// A montagem de verdade: o Jev, com a chave do ambiente ou do
/// `mustard.json` do projeto.
pub(crate) fn jev(root: &Path, config: &ProjectConfig) -> Result<Assembled, FilterError> {
    let loaded = crate::shared::jev::load_key(root, config)?;
    Ok(Assembled {
        name: "jev",
        filter: Box::new(crate::shared::jev::JevFilter::new(loaded.key)),
        warning: loaded.warning,
    })
}

/// Se o aviso do valor inválido de `key` ainda não saiu na sessão `session`;
/// se não saiu, marca que saiu, em `.claude/.session/<sessão>/`. Sem sessão
/// conhecida, avisa sempre: repetir o aviso é melhor que calar o valor que
/// não vale.
pub(crate) fn first_warning(root: &Path, session: Option<&str>, key: &str) -> bool {
    let usable = |s: &&str| !s.is_empty() && *s != "unknown" && !s.starts_with('.') && !s.contains(['/', '\\']);
    let Some(session) = session.map(str::trim).filter(usable) else {
        return true;
    };
    let Ok(paths) = mustard_core::ClaudePaths::for_project(root) else { return true };
    let marker = paths.claude_dir().join(".session").join(session).join(format!("warned-map-{key}"));
    if marker.is_file() {
        return false;
    }
    if let Some(dir) = marker.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&marker, "");
    true
}

/// O filtro da busca, escolhido num ponto só: desligado (`none`) ou com nome
/// desconhecido, nenhum; ausente ou `jev`, o que `assemble` monta, se há
/// chave para o projeto; sem ela, nenhum, com o aviso do motivo. O aviso da
/// chave do `mustard.json` que o git guarda sai também com o filtro montado.
/// Cada aviso sai uma vez por sessão.
pub(crate) fn chosen_filter(
    root: &Path,
    session: Option<&str>,
    lang: Locale,
    config: &ProjectConfig,
    assemble: &Assemble<'_>,
    warnings: &mut Vec<String>,
) -> Option<Assembled> {
    let assembled = match config.search_filter() {
        FilterSetting::Off => None,
        FilterSetting::Invalid => {
            if first_warning(root, session, "search.filter") {
                warnings.push(translate("map.search.bad_filter", lang).to_string());
            }
            None
        }
        FilterSetting::Absent | FilterSetting::Jev => match assemble(root, config) {
            Ok(assembled) => Some(assembled),
            Err(error) => {
                key_warning(root, session, &error, lang, warnings);
                None
            }
        },
    };
    if let Some(error) = assembled.as_ref().and_then(|assembled| assembled.warning.as_ref()) {
        key_warning(root, session, error, lang, warnings);
    }
    assembled
}

/// O aviso da chave do filtro, uma vez por sessão para cada motivo: a chave
/// que falta, ou a do `mustard.json` que o git guarda. Nenhum dos dois leva
/// a chave.
fn key_warning(root: &Path, session: Option<&str>, error: &FilterError, lang: Locale, warnings: &mut Vec<String>) {
    let key = match error {
        FilterError::KeyInGit => "map.search.key_in_git",
        _ => "map.search.missing_key",
    };
    if first_warning(root, session, &format!("search.{}", error.reason())) {
        warnings.push(translate(key, lang).to_string());
    }
}

/// O aviso da falha do filtro, com o motivo, uma vez por sessão.
pub(crate) fn failure_warning(
    root: &Path,
    session: Option<&str>,
    lang: Locale,
    error: &FilterError,
    warnings: &mut Vec<String>,
) {
    if first_warning(root, session, "search.filter_failed") {
        let reason = translate(&format!("map.search.reason.{}", error.reason()), lang);
        warnings.push(translate("map.search.filter_failed", lang).replace("{reason}", reason));
    }
}

/// A busca com o filtro montado: os candidatos do banco, só da pasta e dos
/// tipos de arquivo da busca, vão ao filtro num pedido só, e o que passa do
/// corte volta como peças. Na falha do filtro, o resultado é a falha, e quem
/// chamou responde da triagem.
pub(crate) fn classify(ask: &Ask<'_>, assembled: &Assembled) -> Result<Classified, MapRefusal> {
    let admit = |rel: &str| in_search(rel, ask.rels, ask.filters, ask.walk);
    let found = map_search::candidates(ask.root, ask.query, ask.intent, ask.languages, ask.numbers.candidates, admit)?;
    let words: Vec<String> = ask.query.split_whitespace().map(str::to_string).collect();
    let phrase = phrase_of(&words, ask.query, ask.intent, ask.lang);
    let cut = CutRule {
        share: (ask.numbers.cut_share as f64 / 100.0).min(1.0),
        exists_from: (ask.numbers.exists_from as f64 / 100.0).min(1.0),
        max_kept: ask.numbers.max_kept,
    };
    let request = FilterRequest {
        words,
        phrase,
        described: ask.described.to_string(),
        said: ask.said.to_string(),
        root: ask.root.to_path_buf(),
        cut,
        candidates: found.candidates,
    };
    #[cfg(test)]
    crate::shared::word_search::ruler::jev::remember_candidates(&request.candidates);
    let calling = Instant::now();
    let mut measured = Map::new();
    measured.insert("candidates".to_string(), json!(request.candidates.len()));
    if request.candidates.is_empty() {
        // Sem declaração para classificar, o filtro não tem o que escolher:
        // a resposta é a dos arquivos que a triagem achou.
        measured.insert("filter".to_string(), json!(assembled.name));
        measured.insert("returned".to_string(), json!(ask.triaged.files.len()));
        return Ok(Classified { outcome: Outcome::NoCandidates, measured });
    }
    match assembled.filter.filter(&request) {
        Ok(filtered) => {
            let pieces = pieces(
                ask.root,
                (&request.candidates, &found.whole),
                &filtered,
                (ask.numbers.max_returned, admit),
            )?;
            measured.insert("filter".to_string(), json!(assembled.name));
            measured.insert("filter_ms".to_string(), json!(filtered.usage.millis));
            measured.insert("tokens".to_string(), json!(filtered.usage.input_tokens));
            measured.insert("cost_micro_usd".to_string(), json!(filtered.usage.cost_micro_usd));
            measured.insert("returned".to_string(), json!(pieces.len()));
            if !filtered.usage.model.is_empty() {
                measured.insert("model".to_string(), json!(filtered.usage.model));
            }
            Ok(Classified { outcome: Outcome::Classified { verdict: filtered.verdict, pieces }, measured })
        }
        Err(error) => {
            measured.insert("filter".to_string(), json!(format!("{}:{}", assembled.name, error.reason())));
            measured.insert("filter_ms".to_string(), json!(u64::try_from(calling.elapsed().as_millis()).unwrap_or(u64::MAX)));
            measured.insert("returned".to_string(), json!(ask.triaged.files.len()));
            Ok(Classified { outcome: Outcome::Failed(error), measured })
        }
    }
}

/// As peças da resposta com filtro, na ordem da combinação e até o teto
/// `max`: o que passou do corte, na ordem da chance, e o que cada item dele
/// puxou. Nada do banco entra de fora do corte, e nada que `admit` não deixe
/// entrar: a implementação que um contrato puxaria de outra pasta fica de fora. Cada
/// peça traz o caminho, a linha, o fim, o tipo, o nome, a assinatura e a
/// primeira frase da documentação; só as do corte trazem a chance. Nunca o
/// corpo.
fn pieces(
    root: &Path,
    (candidates, whole): (&[FilterCandidate], &[i64]),
    filtered: &mustard_core::domain::map_filter::Filtered,
    (max, admit): (usize, impl Fn(&str) -> bool),
) -> Result<Vec<Piece>, MapRefusal> {
    let cut: Vec<i64> = filtered.kept.iter().map(|scored| scored.id).collect();
    let scores: HashMap<i64, f64> = filtered.kept.iter().map(|scored| (scored.id, scored.score)).collect();
    let bank: Vec<i64> = candidates.iter().map(|candidate| candidate.id).collect();
    let mut links = map_search::links(root, &cut)?;
    for linked in links.values_mut() {
        linked.implementations.retain(|(_, path)| admit(path));
    }
    let picks = capped(&select(&cut, &bank, whole, &links), max);
    let outside: Vec<i64> = picks.iter().map(|pick| pick.id).filter(|id| !bank.contains(id)).collect();
    let pulled = map_search::declarations(root, &outside)?;
    let known: HashMap<i64, &FilterCandidate> =
        candidates.iter().chain(&pulled).map(|candidate| (candidate.id, candidate)).collect();
    Ok(picks
        .iter()
        .filter_map(|pick| {
            let decl = known.get(&pick.id)?;
            Some(Piece {
                path: decl.path.clone(),
                line: decl.line,
                end_line: decl.end_line,
                kind: decl.kind.clone(),
                name: decl.name.clone(),
                signature: decl.signature.clone(),
                // O começo da documentação: a primeira frase, como a de um
                // item de spec.
                doc: project_map::spec_sentence(&decl.documentation),
                score: (pick.source == Source::Cut).then(|| scores.get(&pick.id).copied()).flatten(),
            })
        })
        .collect())
}
