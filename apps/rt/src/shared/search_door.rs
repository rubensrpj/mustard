//! `search_door` — a porta única da busca do mapa: o caminho que toda busca
//! do Claude faz depois da triagem, seja o pedido por assunto (`run map
//! search`) ou a busca por palavra que o gancho responde (`Grep` e
//! `grep`, `rg` e `git grep` do terminal).
//!
//! O caminho tem quatro passos, e a porta guarda todos:
//!
//! 1. a triagem crava: a resposta sai dela, com a primeira peça inteira, sem
//!    filtro, sem aviso de chave e sem chamada gravada ([`pinned`]);
//! 2. não cravou: a lista inteira de candidatos do banco vai ao filtro, sem
//!    corte, com a história que o mapa guarda de cada um, a frase de quem
//!    procura, a descrição que o agente deu à busca e a última fala dele
//!    ([`classify`]);
//! 3. o filtro diz não achei, e nada volta; ou diz que algum candidato serve, e
//!    voltam todas as peças que passam do corte relativo, sem teto de
//!    quantidade;
//! 4. sem filtro (desligado, sem chave, com o teto de gasto do mês já gasto),
//!    sem candidato ou com o filtro falhando (a chamada que passaria do teto
//!    do mês inclusive), quem chamou responde só com a triagem, e o aviso do
//!    motivo sai uma vez por sessão ([`chosen_filter`], [`failure_warning`]).
//!
//! A porta devolve tipos ([`Outcome`], [`Piece`]); quem chamou monta o texto:
//! o JSON da busca por assunto, em `commands::map`, ou a resposta por função
//! da busca por palavra, em `word_search`. A montagem do filtro ([`Assemble`],
//! [`jev`]) e o aviso de uma vez por sessão ([`first_warning`]) moram aqui,
//! para os dois caminhos os usarem sem um depender do outro.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use mustard_core::domain::map_filter::{CUT_SHARE, CutRule, EXISTS_FROM, FilterCandidate, FilterError, FilterRequest, MapFilter, Verdict};
use mustard_core::domain::map_select::{Source, select};
use mustard_core::domain::normalize::Languages;
use mustard_core::domain::project_map::{self, MapRefusal};
use mustard_core::domain::triage::Mark;
use mustard_core::io::jev_gate::{self, KEY_ENV};
use mustard_core::io::map_search;
use mustard_core::io::map_triage::Triaged;
use mustard_core::platform::i18n::{Locale, translate};
use mustard_core::{FilterSetting, ProjectConfig, Setting};
use serde_json::{Map, Value, json};

use crate::shared::code_route::in_search;
use crate::shared::config_key::{NameFilter, Walk};
use crate::shared::jev_budget::Budget;
use crate::shared::triage_view;

/// Os números da busca com filtro, com o padrão no lugar do ausente e do
/// inválido.
pub(crate) struct Numbers {
    /// A parte da maior chance que passa do corte, em pontos percentuais.
    pub(crate) cut_share: usize,
    /// A chance de algum candidato servir, em pontos percentuais, a partir da
    /// qual há resposta; abaixo dela é não achei.
    pub(crate) exists_from: usize,
}

impl Numbers {
    /// Os números que a seção `search` da configuração diz. O valor inválido
    /// cai no padrão e deixa em `warnings` o aviso que diz a chave e o padrão,
    /// uma vez por sessão.
    pub(crate) fn read(root: &Path, session: Option<&str>, lang: Locale, config: &ProjectConfig, warnings: &mut Vec<String>) -> Self {
        let mut number = |key: &str, setting: Setting, default: usize| {
            if setting == Setting::Invalid && first_warning(root, session, &format!("search.{key}")) {
                warnings.push(translate("map.search.bad_number", lang).replace("{key}", key).replace("{default}", &default.to_string()));
            }
            setting.or(default)
        };
        Self {
            cut_share: number("cut_share", config.search_cut_share(), (CUT_SHARE * 100.0).round() as usize),
            exists_from: number("exists_from", config.search_exists_from(), (EXISTS_FROM * 100.0).round() as usize),
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
pub(crate) fn pinned(root: &Path, (query, intent): (&str, &str), languages: &Languages, triaged: &Triaged) -> Result<Option<FilterCandidate>, MapRefusal> {
    if triaged.mark() != Mark::Pinned {
        return Ok(None);
    }
    triage_view::pinned_piece(root, (query, intent), languages, triaged)
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
    let ledger = mustard_core::io::spend::machine_dir();
    assembled(root, config, std::env::var(KEY_ENV).ok(), ledger.as_deref())
}

/// A montagem do Jev como em [`jev`], com `env` no lugar do valor de
/// [`KEY_ENV`] e o arquivo do gasto em `ledger_dir`: o teste não depende do
/// ambiente de quem o roda. O teto do mês já gasto recusa a montagem com
/// [`FilterError::OverBudget`], como a chave que falta: o resto do que sobra
/// se segura em cada chamada do filtro.
pub(crate) fn assembled(root: &Path, config: &ProjectConfig, env: Option<String>, ledger_dir: Option<&Path>) -> Result<Assembled, FilterError> {
    let loaded = crate::shared::jev::key_in(root, config, env)?;
    let budget = Budget::open(root, config, ledger_dir);
    Ok(Assembled { name: "jev", filter: Box::new(crate::shared::jev::JevFilter::new(root, loaded.key, budget)), warning: loaded.warning })
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
    let Ok(paths) = mustard_core::ClaudePaths::for_project(root) else {
        return true;
    };
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
    let setting = config.judgement_filter("search");
    if setting == FilterSetting::Invalid && first_warning(root, session, "search.filter") {
        warnings.push(translate("map.search.bad_filter", lang).to_string());
    }
    let assembled = if jev_gate::setting_allows(setting) {
        match assemble(root, config) {
            Ok(assembled) => Some(assembled),
            Err(error) => {
                key_warning(root, session, &error, lang, warnings);
                None
            }
        }
    } else {
        None
    };
    if let Some(error) = assembled.as_ref().and_then(|assembled| assembled.warning.as_ref()) {
        key_warning(root, session, error, lang, warnings);
    }
    assembled
}

/// O aviso de por que o filtro não entrou, uma vez por sessão para cada
/// motivo: a chave que falta, a do `mustard.json` que o git guarda ou o teto
/// de gasto do mês. Nenhum deles leva a chave.
fn key_warning(root: &Path, session: Option<&str>, error: &FilterError, lang: Locale, warnings: &mut Vec<String>) {
    let key = match error {
        FilterError::KeyInGit => "map.search.key_in_git",
        FilterError::OverBudget => "map.search.over_budget",
        _ => "map.search.missing_key",
    };
    if first_warning(root, session, &format!("search.{}", error.reason())) {
        warnings.push(translate(key, lang).to_string());
    }
}

/// O aviso da falha do filtro, com o motivo, uma vez por sessão. O teto de
/// gasto do mês tem o aviso dele, o mesmo de quando o filtro nem entrou, e
/// sai uma vez só por sessão, venha de onde vier.
pub(crate) fn failure_warning(root: &Path, session: Option<&str>, lang: Locale, error: &FilterError, warnings: &mut Vec<String>) {
    if *error == FilterError::OverBudget {
        key_warning(root, session, error, lang, warnings);
        return;
    }
    if first_warning(root, session, "search.filter_failed") {
        let reason = translate(&format!("map.search.reason.{}", error.reason()), lang);
        warnings.push(translate("map.search.filter_failed", lang).replace("{reason}", reason));
    }
}

/// A busca com o filtro montado: todos os candidatos do banco, só da pasta e
/// dos tipos de arquivo da busca vão ao filtro. História só entra quando a
/// pergunta pede a origem/evolução; o que passa do corte volta como peças. Na falha do filtro, o resultado é a falha, e quem
/// chamou responde da triagem.
pub(crate) fn classify(ask: &Ask<'_>, assembled: &Assembled) -> Result<Classified, MapRefusal> {
    let admit = |rel: &str| in_search(rel, ask.rels, ask.filters, ask.walk);
    let found = map_search::candidates(ask.root, ask.query, ask.intent, ask.languages, admit)?;
    let local_files: std::collections::BTreeSet<&str> = ask.triaged.files.iter().map(|f| f.path.as_str()).collect();
    let query_forms = mustard_core::domain::normalize::forms(&format!("{} {}", ask.query, ask.intent), ask.languages)
        .into_iter()
        .flatten()
        .collect::<std::collections::BTreeSet<_>>();
    let recovered = found
        .candidates
        .into_iter()
        .filter(|candidate| {
            if local_files.contains(candidate.path.as_str()) {
                return true;
            }
            // The display's TOP is not a retrieval boundary. Keep every native
            // lexical match, including matches below the displayed first page.
            let searchable = format!("{} {} {} {}", candidate.path, candidate.name, candidate.signature, candidate.documentation);
            mustard_core::domain::normalize::forms(&searchable, ask.languages).into_iter().flatten().any(|word| query_forms.contains(&word))
        })
        .collect();
    let candidates = if needs_history(ask.query, ask.intent) { map_search::with_history(ask.root, recovered)? } else { recovered };
    let words: Vec<String> = ask.query.split_whitespace().map(str::to_string).collect();
    let phrase = phrase_of(&words, ask.query, ask.intent, ask.lang);
    let cut = CutRule { share: (ask.numbers.cut_share as f64 / 100.0).min(1.0), exists_from: (ask.numbers.exists_from as f64 / 100.0).min(1.0) };
    let request =
        FilterRequest { words, phrase, described: ask.described.to_string(), said: ask.said.to_string(), root: ask.root.to_path_buf(), cut, candidates };
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
            let pieces = pieces(ask.root, &request.candidates, &filtered, admit)?;
            measured.insert("filter".to_string(), json!(assembled.name));
            measured.insert("filter_ms".to_string(), json!(filtered.usage.millis));
            if !filtered.usage.incomplete {
                measured.insert("tokens".to_string(), json!(filtered.usage.input_tokens));
                measured.insert("cost_micro_usd".to_string(), json!(filtered.usage.cost_micro_usd));
            }
            measured.insert("requests".to_string(), json!(filtered.usage.requests));
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

/// Current-code location does not require shipping all historical commits.
/// Explicit origin/evolution questions retain complete relevant history.
fn needs_history(query: &str, intent: &str) -> bool {
    let text = mustard_core::domain::text::fold_accents(&format!("{query} {intent}").to_lowercase());
    let words: std::collections::BTreeSet<_> = text.split(|c: char| !c.is_alphanumeric()).collect();
    text.contains("por que")
        || [
            "why",
            "history",
            "historical",
            "historia",
            "historico",
            "origem",
            "motivo",
            "justificativa",
            "revert",
            "reverter",
            "rollback",
            "previous",
            "anterior",
            "antes",
        ]
        .iter()
        .any(|word| words.contains(word))
}

/// As peças da resposta com filtro, na ordem da combinação, sem teto de
/// quantidade: tudo o que passou do corte, na ordem da chance, e o que cada
/// item dele puxou. Nada do banco entra de fora do corte, e nada que `admit` não deixe
/// entrar: a implementação que um contrato puxaria de outra pasta fica de fora. Cada
/// peça traz o caminho, a linha, o fim, o tipo, o nome, a assinatura e a
/// primeira frase da documentação; só as do corte trazem a chance. Nunca o
/// corpo.
fn pieces(
    root: &Path,
    candidates: &[FilterCandidate],
    filtered: &mustard_core::domain::map_filter::Filtered,
    admit: impl Fn(&str) -> bool,
) -> Result<Vec<Piece>, MapRefusal> {
    let cut: Vec<i64> = filtered.kept.iter().map(|scored| scored.id).collect();
    let scores: HashMap<i64, f64> = filtered.kept.iter().map(|scored| (scored.id, scored.score)).collect();
    let bank: Vec<i64> = candidates.iter().map(|candidate| candidate.id).collect();
    let mut links = map_search::links(root, &cut)?;
    for linked in links.values_mut() {
        linked.implementations.retain(|(_, path)| admit(path));
    }
    let picks = select(&cut, &bank, &links);
    let outside: Vec<i64> = picks.iter().map(|pick| pick.id).filter(|id| !bank.contains(id)).collect();
    let pulled = map_search::declarations(root, &outside)?;
    let known: HashMap<i64, &FilterCandidate> = candidates.iter().chain(&pulled).map(|candidate| (candidate.id, candidate)).collect();
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

#[cfg(test)]
mod tests {
    use mustard_core::domain::spend::DayRow;
    use mustard_core::io::spend;

    use super::*;

    /// Com sobra no mês, a montagem entrega o filtro; sem sobra, recusa com o
    /// teto, e o gasto de outro mês não conta. O gasto vem das specs do projeto
    /// e dos outros projetos do arquivo do gasto da máquina; o padrão do teto é
    /// de 10 dólares, e o teto que não vale (texto, negativo) cai nele.
    #[test]
    fn the_filter_is_built_only_while_the_month_has_budget_left() {
        // Um projeto com o `mustard.json` `config` e, quando `call_cost` vem,
        // uma chamada ao Jev de hoje com esse custo gravada na spec dele.
        let project = |config: &str, call_cost: Option<u64>| {
            let root = tempfile::tempdir().unwrap();
            std::fs::write(root.path().join("mustard.json"), config).unwrap();
            if let Some(cost) = call_cost {
                let spec = root.path().join(".claude/spec/uma-spec");
                std::fs::create_dir_all(&spec).unwrap();
                let at = format!("{}T12:00:00-03:00", spend::today());
                let event = json!({"v": 1, "id": 1, "code": "X-CALL-0001", "at": at, "type": "call", "author": "binary",
                    "command": "map search", "filter": "jev", "tokens": 1000, "cost_micro_usd": cost});
                std::fs::write(spec.join("spec.ndjson"), format!("{event}\n")).unwrap();
            }
            root
        };
        let built = |root: &tempfile::TempDir, ledger_dir: Option<&Path>| {
            let config = ProjectConfig::load(root.path());
            assembled(root.path(), &config, Some("from-env".to_string()), ledger_dir)
        };
        // O arquivo do gasto da máquina com `rows` (dia, projeto, custo do Jev).
        let ledger = |rows: &[(String, &str, u64)]| {
            let dir = tempfile::tempdir().unwrap();
            spend::update(dir.path(), |ledger| {
                for (day, project, cost) in rows {
                    ledger.rows.push(DayRow { day: day.clone(), project: project.to_string(), jev_cost_micro_usd: *cost, ..DayRow::default() });
                }
            })
            .unwrap();
            dir
        };
        let first_day = format!("{}-01", spend::this_month());

        // O padrão de 10 dólares: 9,99 de gasto deixa sobra, 10 não; o teto que
        // não vale cai no padrão.
        assert!(built(&project("{}", Some(9_990_000)), None).is_ok(), "below the default budget");
        assert!(built(&project("{}", Some(10_000_000)), None).is_ok());
        for invalid in [r#""2""#, "-2"] {
            let config = format!(r#"{{"jev": {{"monthly_budget_usd": {invalid}}}}}"#);
            assert!(built(&project(&config, Some(10_000_000)), None).is_ok(), "{invalid}");
        }

        // O teto do `mustard.json` vale no lugar do padrão.
        let low = r#"{"jev": {"monthly_budget_usd": 2}}"#;
        assert!(built(&project(low, Some(1_000_000)), None).is_ok());
        assert!(built(&project(low, Some(2_000_000)), None).is_ok());

        // Os outros projetos da máquina gastam do mesmo teto; o mês passado e
        // o próprio projeto no arquivo não contam.
        let root = project("{}", None);
        let own = root.path().file_name().unwrap().to_str().unwrap().to_string();
        let elsewhere =
            ledger(&[(first_day.clone(), "outro", 9_000_000), ("2000-01-15".to_string(), "outro", 50_000_000), (first_day.clone(), own.as_str(), 50_000_000)]);
        assert!(built(&root, Some(elsewhere.path())).is_ok(), "9 dollars elsewhere this month, 50 in an old month and in its own rows");
        let more = ledger(&[(first_day, "outro", 10_000_000)]);
        assert!(built(&root, Some(more.path())).is_ok());
    }

    /// O mês gasto vale como sem chave: o filtro não entra e o aviso do teto
    /// sai uma vez por sessão — na montagem e na falha da chamada que passaria
    /// do teto, juntas —, e sem sessão sai sempre.
    #[test]
    fn the_budget_warning_comes_once_per_session() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("mustard.json"), r#"{"jev": {"monthly_budget_usd": 0}}"#).unwrap();
        let config = ProjectConfig::load(root.path());
        let assemble = |at: &Path, config: &ProjectConfig| assembled(at, config, Some("from-env".to_string()), None);
        let mut warnings = Vec::new();
        let chose = |session: Option<&str>, warnings: &mut Vec<String>| {
            let has_adapter = chosen_filter(root.path(), session, Locale::PtBr, &config, &assemble, warnings).is_some();
            // Only an actual cache miss that the reservation refused warns.
            failure_warning(root.path(), session, Locale::PtBr, &FilterError::OverBudget, warnings);
            has_adapter
        };

        assert!(chose(Some("s1"), &mut warnings), "cache lookup remains available once the month is spent");
        assert_eq!(warnings, vec![translate("map.search.over_budget", Locale::PtBr).to_string()]);
        assert!(chose(Some("s1"), &mut warnings));
        failure_warning(root.path(), Some("s1"), Locale::PtBr, &FilterError::OverBudget, &mut warnings);
        assert_eq!(warnings.len(), 1, "the same session hears it once, from either place");
        assert!(chose(Some("s2"), &mut warnings));
        assert_eq!(warnings.len(), 2, "a new session hears it again");
        assert!(chose(None, &mut warnings) && chose(None, &mut warnings));
        assert_eq!(warnings.len(), 4, "without a session it always comes");
    }
}
