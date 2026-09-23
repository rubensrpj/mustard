//! Os templates das páginas do Mustard: a página de uma spec e a página do
//! projeto.
//!
//! Cada template é um HTML pronto, com o visual do Mustard (mostarda e
//! carvão), igual para toda spec e para todo projeto. Ele é publicado uma vez
//! só no claude.ai e, ao abrir, lê o banco de dados que o claude.ai guarda
//! junto da página publicada: o binário não monta mais a página, só prepara
//! os dados. Os dois arquivos moram em `packages/core/templates/pages/` e vão
//! embutidos no binário; o estilo e o script deles mudam sem mexer no código
//! Rust.
//!
//! ## O que o template recebe do binário
//!
//! O template tem um lugar vazio para o catálogo, a tag
//! `<script type="application/json" id="mustard-catalog">{}</script>`.
//! [`spec_page_template`] e [`project_page_template`] o preenchem com o que o
//! binário sabe e o banco não traz: os textos da página no idioma do projeto,
//! lidos do catálogo de textos, e, na página da spec, os tipos de evento
//! (sigla do código, bloco e campos de cada um), os nomes dos blocos e das
//! fases e a marca do expurgo. Assim o template não guarda uma segunda cópia
//! dessas regras. Os
//! textos que o catálogo leva são os que o próprio template cita entre aspas
//! simples (`'page.loading'`), mais os nomes dos tipos, dos campos, dos
//! valores, das fases, dos autores e dos blocos.
//!
//! ## O banco de dados da página da spec
//!
//! - A coleção [`RANGES`] tem um documento por faixa fixa de [`RANGE_WIDTH`]
//!   números (os itens de 2000 a 2099 vão no documento `2000`, por exemplo),
//!   com `seq` (a ordem da faixa, e do pedaço dela) e `items` (a lista dos
//!   itens da faixa, cada um como a linha do arquivo, com `id`, `code`,
//!   `at`, `type`, `author` e os campos do tipo). A faixa que passar de
//!   [`RANGE_MAX_BYTES`] se parte em pedaços, dentro dela mesma, cada um com
//!   o próprio documento. O template lê as faixas em ordem, abre a lista de
//!   itens de cada documento e monta a lista de itens em páginas.
//! - O documento [`COMPUTED`] guarda o que o binário calcula e o
//!   `spec.ndjson` não tem, trocado a cada cópia: `spec` (o nome da spec),
//!   `last` (o número do último item copiado, que muda em toda cópia, para a
//!   página aberta sempre notar algo novo, mesmo quando só um item do meio
//!   saiu), `waves` (o estado de cada onda pelo número dela: `todo`,
//!   `running`, `delivered`, `approved` ou `rejected`), `prompts` (o pedido de
//!   cada onda que ainda não saiu, pelo número dela) e `rtk` (a economia do
//!   rtk, um dia por linha, com `date`, `commands`, `input` e `saved`).
//!
//! O template mostra a versão mais nova de cada item, esconde o item
//! retirado, marca o que entrou depois da aprovação e mostra o pedido de cada
//! onda com o texto de cada item no lugar do código. No fim, a seção
//! Removidos lista cada item que saiu, com o código, o texto, quem o tirou,
//! quando e por quê; o item expurgado aparece só com a marca do expurgo
//! ([`PURGED_MARK`], que o catálogo leva) no lugar do texto, na tela e no
//! `.md` baixado. Ele se atualiza sozinho
//! quando o documento das coisas calculadas ou o item mais novo mudam. Sem
//! banco, ou com o banco vazio, ele diz que ainda não há dados.
//!
//! ## O banco de dados da página do projeto
//!
//! A coleção [`SPECS`] tem um documento por spec, com o nome da spec como
//! nome do documento e, no corpo, `name`, `goal` (o objetivo), `phase`,
//! `branch`, `created`, `updated` e `url` (o link da página da spec, quando
//! ela já foi publicada).
//!
//! ## Na publicação
//!
//! A página só alcança o banco quando declara o uso dele ao ser publicada:
//! [`SPEC_CAPABILITIES`] e [`PROJECT_CAPABILITIES`] são as declarações de
//! cada uma. Só quem pode editar a página grava no banco; quem só abre a
//! página lê.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

use crate::domain::spec_events::{Block, Kind, AUTHORS, PHASES, PURGED_MARK, TYPES};
use crate::domain::spec_state::is_approved_phase;
use crate::platform::harness::harness_version;
use crate::platform::i18n::{translate, Locale};

/// O template da página da spec, como mora no repositório.
const SPEC_PAGE: &str = include_str!("../../templates/pages/spec.html");

/// O template da página do projeto, como mora no repositório.
const PROJECT_PAGE: &str = include_str!("../../templates/pages/project.html");

/// O lugar do catálogo em cada template, vazio até o binário preenchê-lo.
const CATALOG_SLOT: &str = r#"<script type="application/json" id="mustard-catalog">{}</script>"#;

/// A coleção das faixas de itens da spec no banco da página dela.
pub const RANGES: &str = "ranges";

/// Quantos números cabem numa faixa da coleção [`RANGES`]: os itens de 2000
/// a 2099 vão no documento `2000`, por exemplo.
pub const RANGE_WIDTH: u64 = 100;

/// O tamanho, em bytes do JSON, que faz uma faixa se partir em pedaços, cada
/// um com o nome do início seguido do número do pedaço (`2000-2`, `2000-3`…).
pub const RANGE_MAX_BYTES: usize = 200 * 1024;

/// O documento das coisas calculadas no banco da página da spec.
pub const COMPUTED: &str = "computed/current";

/// A coleção das specs no banco da página do projeto.
pub const SPECS: &str = "specs";

/// O que a página da spec declara ao ser publicada: o banco de dados, que só
/// quem edita a página grava, e o salvar arquivo do botão de baixar o `.md`.
pub const SPEC_CAPABILITIES: &str =
    r#"{"db":{"rules":[{"path":"","read":"view","write":"admin"}]},"downloads":true}"#;

/// O que a página do projeto declara ao ser publicada: o banco de dados, que
/// só quem edita a página grava.
pub const PROJECT_CAPABILITIES: &str = r#"{"db":{"rules":[{"path":"","read":"view","write":"admin"}]}}"#;

/// O template da página da spec, com o catálogo no idioma `lang`.
#[must_use]
pub fn spec_page_template(lang: Locale) -> String {
    let finding_labels = [Locale::PtBr, Locale::EnUs].map(|l| translate("plan.finding.label", l));
    let catalog = json!({
        "lang": lang.as_str(),
        "db": { "ranges": RANGES, "computed": COMPUTED },
        "types": types(),
        "blocks": Block::ALL.iter().map(|b| b.name()).collect::<Vec<_>>(),
        "phases": PHASES,
        "approvedPhases": PHASES.iter().filter(|p| is_approved_phase(p)).collect::<Vec<_>>(),
        "findingLabels": finding_labels,
        "purgedMark": PURGED_MARK,
        "labels": labels(SPEC_PAGE, lang),
    });
    fill(SPEC_PAGE, &catalog)
}

/// O template da página do projeto, com o catálogo no idioma `lang`.
#[must_use]
pub fn project_page_template(lang: Locale) -> String {
    let catalog = json!({
        "lang": lang.as_str(),
        "db": { "specs": SPECS },
        "phases": PHASES,
        "labels": labels(PROJECT_PAGE, lang),
    });
    fill(PROJECT_PAGE, &catalog)
}

/// O template com o catálogo no lugar dele. O catálogo vai como JSON dentro
/// de uma tag `<script>`: todo `<` vira o escape `\u003c` do JSON, para nenhum texto
/// fechar a tag antes da hora.
fn fill(template: &str, catalog: &Value) -> String {
    let json = catalog.to_string().replace('<', "\\u003c");
    let filled = format!(r#"<script type="application/json" id="mustard-catalog">{json}</script>"#);
    let body = template.replacen(CATALOG_SLOT, &filled, 1);
    stamp(&body)
}

/// O que vem antes do carimbo na marca do começo do modelo, na primeira
/// linha dele.
const VERSION_MARK_PREFIX: &str = "<!-- mustard:";

/// O que vem depois do carimbo na marca do começo do modelo.
const VERSION_MARK_SUFFIX: &str = "-->";

/// `body` com o carimbo do modelo na frente, numa linha só: a versão do
/// Mustard rodando e a impressão do conteúdo montado, o template com o
/// catálogo já no lugar. É pelo carimbo inteiro que o passo que publica
/// compara o modelo instalado no projeto e o da última publicação com o que
/// o binário monta agora, sem reconstruir o catálogo inteiro: dois programas
/// com a mesma versão e moldes diferentes, um instalado e outro compilado no
/// meio de uma obra, dão carimbos diferentes.
fn stamp(body: &str) -> String {
    format!("{VERSION_MARK_PREFIX} {} {:016x} {VERSION_MARK_SUFFIX}\n{body}", harness_version(), fingerprint(body))
}

/// A impressão do conteúdo `body`: o FNV-1a de 64 bits sobre os bytes dele,
/// estável entre versões do Rust e entre máquinas, ao contrário do hasher da
/// biblioteca padrão. Qualquer mudança no template ou no catálogo muda a
/// impressão.
fn fingerprint(body: &str) -> u64 {
    body.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3))
}

/// O carimbo do modelo `text`, lido da primeira linha: a versão do Mustard
/// que o gerou e a impressão do conteúdo, juntas, como a publicação o grava.
/// `None` quando a marca não está lá — um modelo de antes dela, ou qualquer
/// outro texto. O modelo de antes da impressão tem só a versão no carimbo, e
/// por isso nunca é igual ao de agora.
#[must_use]
pub fn template_stamp(text: &str) -> Option<&str> {
    let line = text.lines().next()?;
    let rest = line.strip_prefix(VERSION_MARK_PREFIX)?.trim();
    rest.strip_suffix(VERSION_MARK_SUFFIX).map(str::trim)
}

/// Cada tipo de evento como o template o lê: o nome, a sigla do código, o
/// bloco e os campos, cada um com o nome e a forma.
fn types() -> Vec<Value> {
    TYPES
        .iter()
        .map(|spec| {
            let fields: Vec<Value> =
                spec.fields.iter().map(|f| json!({ "name": f.name, "kind": kind_name(f.kind) })).collect();
            json!({ "name": spec.name, "code": spec.code, "block": spec.block.name(), "fields": fields })
        })
        .collect()
}

/// O nome da forma de um campo no catálogo do template.
fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Text => "text",
        Kind::Int => "int",
        Kind::Bool => "bool",
        Kind::Object => "object",
        Kind::Ints => "ints",
        Kind::Texts => "texts",
        Kind::Objects => "objects",
        Kind::List => "list",
        Kind::OneOf(_) => "one_of",
        Kind::ManyOf(_) => "many_of",
        Kind::OneOfNumbers(_) => "one_of_numbers",
        Kind::TextOrObject => "text_or_object",
        Kind::Time => "time",
        Kind::Ref => "ref",
        Kind::Refs => "refs",
    }
}

/// Os textos que o template usa, no idioma `lang`: os que ele cita entre
/// aspas simples e os nomes que ele monta a partir dos tipos. Uma chave sem
/// texto no catálogo fica de fora, e o template mostra a própria chave.
fn labels(template: &str, lang: Locale) -> BTreeMap<String, &'static str> {
    let mut keys = cited_keys(template);
    keys.extend(named_keys());
    keys.into_iter()
        .filter_map(|key| {
            let text = translate(&key, lang);
            (text != "<missing-key>").then_some((key, text))
        })
        .collect()
}

/// As chaves do catálogo que o template cita entre aspas simples, como
/// `'page.loading'`: cada aspa seguida de `page.` ou `project.`, até a aspa
/// que fecha, só com letras minúsculas, números, ponto e sublinhado.
fn cited_keys(template: &str) -> BTreeSet<String> {
    template
        .match_indices('\'')
        .filter_map(|(at, _)| {
            let rest = &template[at + 1..];
            let end = rest.find('\'')?;
            let quoted = &rest[..end];
            let known = quoted.starts_with("page.") || quoted.starts_with("project.");
            let plain = quoted.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'_');
            (known && plain).then(|| quoted.to_string())
        })
        .collect()
}

/// Os nomes que o template monta a partir dos tipos: o de cada bloco, tipo,
/// grupo, campo, valor, fase e autor.
fn named_keys() -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    for block in Block::ALL {
        keys.insert(format!("page.block.{}", block.name()));
    }
    for spec in TYPES {
        keys.insert(format!("page.type.{}", spec.name));
        keys.insert(format!("page.group.{}", spec.name));
        for field in spec.fields {
            keys.insert(format!("page.field.{}", field.name));
            if let Kind::OneOf(words) | Kind::ManyOf(words) = field.kind {
                let prefix = if field.name == "phase" { "page.phase" } else { "page.value" };
                keys.extend(words.iter().map(|word| format!("{prefix}.{word}")));
            }
        }
    }
    keys.extend(PHASES.iter().map(|phase| format!("page.phase.{phase}")));
    keys.extend(AUTHORS.iter().map(|author| format!("page.author.{author}")));
    keys
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    use serde_json::{json, Value};

    use std::collections::BTreeMap;

    use super::*;
    use crate::domain::spec_events::parse_log;
    use crate::domain::spec_index::ProjectRow;
    use crate::view::document::{RtkDay, WaveState, WaveStates};

    /// O modelo gerado leva na primeira linha o carimbo: a versão do binário
    /// rodando e a impressão do conteúdo montado. É essa marca que o passo
    /// que publica confere para saber se o modelo instalado no projeto, ou o
    /// da página publicada, está velho.
    #[test]
    fn the_generated_template_is_stamped_with_the_version_and_the_content() {
        for html in [spec_page_template(Locale::PtBr), project_page_template(Locale::PtBr)] {
            let stamp = template_stamp(&html).expect("the stamp");
            let (version, print) = stamp.split_once(' ').expect("the version and the fingerprint");
            assert_eq!(version, harness_version(), "{stamp}");
            let body = html.split_once('\n').map(|(_, body)| body).expect("the body after the stamp");
            assert_eq!(print, format!("{:016x}", fingerprint(body)), "{stamp}");
        }
    }

    /// Com a mesma versão, um molde de conteúdo diferente dá outro carimbo:
    /// o idioma do catálogo muda o conteúdo, e muda o carimbo; o mesmo molde
    /// montado duas vezes dá o mesmo carimbo.
    #[test]
    fn the_same_version_with_another_content_has_another_stamp() {
        let pt = spec_page_template(Locale::PtBr);
        let en = spec_page_template(Locale::EnUs);
        assert_ne!(template_stamp(&pt), template_stamp(&en), "the content differs");
        assert_eq!(template_stamp(&pt), template_stamp(&spec_page_template(Locale::PtBr)), "the same content");
        assert_ne!(stamp("a"), stamp("b"), "one byte is enough");
    }

    /// Um modelo sem a marca — de antes dela, ou qualquer outro texto — não
    /// tem carimbo nenhum: a leitura não inventa um. O de antes da impressão
    /// tem só a versão, e por isso não é o carimbo de agora.
    #[test]
    fn a_template_without_the_stamp_has_no_stamp() {
        assert_eq!(template_stamp("<!doctype html><html></html>"), None);
        assert_eq!(template_stamp(""), None);
        let only_version = format!("<!-- mustard: {} -->\n<html></html>", harness_version());
        assert_eq!(template_stamp(&only_version), Some(harness_version().as_str()));
        assert_ne!(template_stamp(&only_version), template_stamp(&spec_page_template(Locale::PtBr)));
    }

    /// O apoio que roda um template no Node, com o DOM e as capacidades do
    /// claude.ai imitados.
    const HARNESS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/page_templates/harness.js");

    /// A spec de exemplo da página de hoje, com um item de cada tipo.
    const FIXTURE: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../apps/rt/tests/fixtures/spec_page/spec.ndjson"));

    /// As seções, os grupos e os itens que o motor antigo (`spec_page`, saído
    /// nesta obra) montava para a spec de exemplo: gravados uma vez, à mão,
    /// como arquivo fixo — o teste lado a lado não chama mais o motor antigo.
    const SPEC_PAGE_FIXTURE: &str =
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/page_templates/spec_page.json"));

    /// As mesmas linhas que o motor antigo (`project_document`, saído nesta
    /// obra) montava para a página do projeto de exemplo, como arquivo fixo.
    const PROJECT_PAGE_FIXTURE: &str =
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/page_templates/project_page.json"));

    /// Os registros internos que a cópia para o banco não leva.
    const INTERNAL: &[&str] = &["injection", "hook", "call"];

    /// O pedido da onda 3, como o agente o recebe: só os códigos.
    const WAVE_3_PROMPT: &str = "# demo — onda 3\n\n## Especificação\n\n- `specification`: MSTD-CTX-0001, MSTD-CONC-0001\n\n## Requisitos acordados\n\n- `agreed`: MSTD-RULE-0001, MSTD-DEC-0001\n\n## Critérios\n\n- `criteria`: MSTD-CRIT-0001\n";

    /// A spec de exemplo mais o que a página nova precisa mostrar: uma regra
    /// revista com duas linhas de lista, uma onda enviada no formato de hoje
    /// e uma onda ainda por enviar.
    fn spec_lines() -> Vec<Value> {
        let extra = [
            json!({"v":1,"id":40,"at":"2026-09-12T12:00:00-03:00","type":"wave","author":"assistant","n":3,"text":"Os templates leem o banco.","criteria":[19],"done_when":"A suíte passa.","origin":2}),
            json!({"v":1,"id":41,"at":"2026-09-12T12:01:00-03:00","type":"task","author":"assistant","wave":3,"text":"Template da página da spec.","files":[{"path":"packages/core/templates/pages/spec.html","new":true}],"origin":2}),
            json!({"v":1,"id":42,"at":"2026-09-12T12:02:00-03:00","type":"rule","author":"assistant","text":"A trava de comandos confere o programa, as opções e o caminho.\n\n- vale para o Bash;\n- vale para o PowerShell.","example":"`rm -rf pasta` é barrado.","keys":["trava"],"origin":2,"replaces":9}),
            json!({"v":1,"id":43,"at":"2026-09-12T12:03:00-03:00","type":"send","author":"binary","wave":3,"role":"wave","text":WAVE_3_PROMPT,"lines":13,"chars":220,"items":[17,18,42,16,19],"mustard":"0.2.1",
                "analysis":{"judged":[17,42],
                    "removed":[{"item":42,"why":"A regra fala da trava, não da tabela desta onda."}],
                    "added":[{"item":17,"why":"O contexto explica por que a tabela nasce vazia."}],
                    "judged_lessons":[],
                    "removed_lessons":[{"lesson":12,"why":"A lição é de outra onda."}],
                    "tasks":[]}}),
            json!({"v":1,"id":44,"at":"2026-09-12T12:04:00-03:00","type":"wave","author":"assistant","n":4,"text":"A instalação.","criteria":[19],"done_when":"O instalador passa.","origin":2}),
            // O veredito final do agente de teste dedicado, separado dos
            // vereditos de onda mesmo apontando a mesma onda 2.
            json!({"v":1,"id":46,"at":"2026-09-12T12:05:00-03:00","type":"verdict","author":"review","wave":2,"result":"approved","final":true,"text":"As ondas se encaixam sem prova perdida."}),
        ];
        FIXTURE
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str::<Value>(line).expect("a fixture line"))
            .chain(extra)
            .filter(|event| !INTERNAL.contains(&event["type"].as_str().unwrap_or_default()))
            .collect()
    }

    fn wave_states() -> WaveStates {
        WaveStates::from([(2, WaveState::Approved), (3, WaveState::Running)])
    }

    fn wave_prompts() -> BTreeMap<u64, String> {
        BTreeMap::from([
            (1, "# demo — onda 1\n\n## A onda e as tarefas dela\n\n- `waves`: MSTD-WAVE-0001\n".to_string()),
            (4, "# demo — onda 4\n\n## Requisitos acordados\n\n- `agreed`: MSTD-RULE-0001\n".to_string()),
        ])
    }

    fn rtk_days() -> Vec<RtkDay> {
        vec![RtkDay { date: "2026-09-12".into(), commands: 40, input: 10_000, saved: 8_000 }]
    }

    /// O nome do estado da onda no documento das coisas calculadas.
    fn state_name(state: WaveState) -> &'static str {
        match state {
            WaveState::Todo => "todo",
            WaveState::Running => "running",
            WaveState::Approved => "approved",
            WaveState::Rejected => "rejected",
        }
    }

    /// Os documentos da coleção das faixas, como a cópia os deixa: um por
    /// faixa de [`RANGE_WIDTH`] números, com o início dela como nome e a
    /// lista dos itens da faixa, na ordem, em `items`.
    fn range_docs(lines: &[Value]) -> Vec<Value> {
        let mut by_start: BTreeMap<u64, Vec<Value>> = BTreeMap::new();
        for line in lines {
            let start = (line["id"].as_u64().unwrap_or(0) / RANGE_WIDTH) * RANGE_WIDTH;
            by_start.entry(start).or_default().push(line.clone());
        }
        by_start
            .into_iter()
            .map(|(start, items)| json!({"id": start.to_string(), "data": {"seq": start * 1000, "items": items}}))
            .collect()
    }

    /// O banco da página da spec, como a cópia o deixa: um documento por
    /// faixa de itens, com o início dela como nome, e o documento das
    /// coisas calculadas.
    fn spec_database(lines: &[Value]) -> Value {
        let waves: serde_json::Map<String, Value> =
            wave_states().into_iter().map(|(n, s)| (n.to_string(), json!(state_name(s)))).collect();
        let prompts: serde_json::Map<String, Value> =
            wave_prompts().into_iter().map(|(n, p)| (n.to_string(), json!(p))).collect();
        let rtk: Vec<Value> = rtk_days()
            .into_iter()
            .map(|d| json!({"date": d.date, "commands": d.commands, "input": d.input, "saved": d.saved}))
            .collect();
        json!({
            "ranges": range_docs(lines),
            "computed": [{"id": "current", "data": {"spec": "demo", "waves": waves, "prompts": prompts, "rtk": rtk}}],
        })
    }

    /// Roda `html` no Node com o banco `db` e os passos `steps`, com o
    /// salvar arquivo do claude.ai (`downloads`) disponível, e devolve o que
    /// a página mostrou em cada passo.
    fn run(page: &str, html: &str, db: Option<Value>, steps: Value) -> Value {
        run_with_downloads(page, html, db, steps, true)
    }

    /// [`run`] com controle sobre o salvar arquivo do claude.ai
    /// (`downloads`): sem ele, o botão de baixar não pode nem tentar o
    /// navegador direto, porque isso não funciona dentro do claude.ai.
    fn run_with_downloads(page: &str, html: &str, db: Option<Value>, steps: Value, downloads: bool) -> Value {
        let mut child = Command::new("node")
            .arg(HARNESS)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the page templates run in Node.js during the test: install Node and put `node` on the PATH");
        let input = json!({"page": page, "html": html, "db": db, "steps": steps, "downloads": downloads}).to_string();
        child.stdin.take().expect("stdin").write_all(input.as_bytes()).expect("the harness reads its input");
        let out = child.wait_with_output().expect("the harness ends");
        assert!(out.status.success(), "the harness failed: {}", String::from_utf8_lossy(&out.stderr));
        let got: Value = serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stdout)));
        assert_eq!(got["errors"], json!([]), "the page script failed");
        got
    }

    /// A aba `anchor` do painel, como a página a mostra.
    fn panel<'a>(seen: &'a Value, anchor: &str) -> &'a Value {
        seen["sections"]
            .as_array()
            .expect("sections")
            .iter()
            .find(|s| s["id"] == json!(anchor))
            .unwrap_or_else(|| panic!("no {anchor} tab: {}", seen["sections"]))
    }

    /// Os cartões da aba `anchor`, na ordem.
    fn cards<'a>(seen: &'a Value, anchor: &str) -> &'a Vec<Value> {
        panel(seen, anchor)["items"].as_array().expect("items")
    }

    /// Um cartão na forma que o arquivo fixo guarda.
    fn card_seen(i: &Value) -> Value {
        let fields: Vec<Value> = i["fields"].as_array().expect("fields").iter().map(|f| json!([f[0], f[1]])).collect();
        json!({"code": i["code"], "anchored": i["anchored"], "title": i["title"], "who": i["who"],
            "mark": i["mark"], "status": i["status"], "date": i["date"], "fields": fields})
    }

    /// Os cartões de cada aba, menos a dos removidos, que a página do motor
    /// antigo não tinha; em cada aba, pela ordem do código e da data.
    fn page_seen(seen: &Value) -> Value {
        let tabs: Vec<Value> = seen["sections"]
            .as_array()
            .expect("sections")
            .iter()
            .filter(|s| s["id"] != json!("removed"))
            .map(|s| {
                let mut items: Vec<Value> = s["items"].as_array().expect("items").iter().map(card_seen).collect();
                items.sort_by_key(|i| (i["code"].to_string(), i["date"].to_string()));
                json!({"id": s["id"], "items": items})
            })
            .collect();
        json!(tabs)
    }

    /// O arquivo fixo do motor antigo lido pelas abas do painel: cada item
    /// vai para a aba do bloco dele; o primeiro contexto sai para o
    /// Objetivo; o que é de uma onda conhecida (a onda, as tarefas, o
    /// envio, a entrega e o commit) sai das abas para o detalhe dela; o
    /// achado do plano fica nas Anotações e a skill no Andamento; e o
    /// critério mostra a prova num selo, no lugar da linha da última
    /// execução, com as execuções dentro do cartão dele.
    fn fixed_tabs() -> Value {
        let fixed: Value = serde_json::from_str(SPEC_PAGE_FIXTURE).expect("the spec page fixture is valid JSON");
        let tab_of = |section: &str, group: &str| -> Option<&'static str> {
            match (section, group) {
                ("waves", "waves-skills") => Some("progress"),
                ("waves", _) | ("criteria", "criteria-runs") => None,
                ("findings", _) => Some("notes"),
                ("progress", _) => Some("progress"),
                ("specification", _) => Some("specification"),
                ("agreed", _) => Some("agreed"),
                ("criteria", _) => Some("criteria"),
                ("review", _) => Some("review"),
                ("notes", _) => Some("notes"),
                ("conversation", _) => Some("conversation"),
                _ => panic!("a fixed section with no tab: {section}"),
            }
        };
        let order = ["specification", "agreed", "criteria", "notes", "review", "progress", "conversation"];
        let mut by_tab: BTreeMap<&str, Vec<Value>> = order.iter().map(|t| (*t, Vec::new())).collect();
        for section in fixed.as_array().expect("sections") {
            for group in section["groups"].as_array().expect("groups") {
                let Some(tab) = tab_of(section["id"].as_str().unwrap_or_default(), group["id"].as_str().unwrap_or_default())
                else {
                    continue;
                };
                for item in group["items"].as_array().expect("items") {
                    let code = item["code"].as_str().unwrap_or_default();
                    if ["MSTD-CTX-0001", "MSTD-COMMIT-0001"].contains(&code) {
                        continue;
                    }
                    let mut item = item.clone();
                    if code == "MSTD-CRIT-0001" {
                        item["status"] = json!("verde");
                        item["fields"] = json!(item["fields"]
                            .as_array()
                            .expect("fields")
                            .iter()
                            .filter(|f| f[0] != json!("Última execução"))
                            .cloned()
                            .collect::<Vec<_>>());
                    }
                    by_tab.get_mut(tab).expect("a tab").push(item);
                }
            }
        }
        json!(order
            .iter()
            .map(|tab| {
                let mut items = by_tab.remove(tab).unwrap_or_default();
                items.sort_by_key(|i| (i["code"].to_string(), i["date"].to_string()));
                json!({"id": tab, "items": items})
            })
            .collect::<Vec<_>>())
    }

    /// O código de cada cartão à mostra, de todas as abas.
    fn visible_codes(seen: &Value) -> Vec<String> {
        seen["sections"]
            .as_array()
            .expect("sections")
            .iter()
            .flat_map(|s| s["items"].as_array().expect("items").iter())
            .filter(|i| i["hidden"] == json!(false))
            .map(|i| i["code"].as_str().unwrap_or_default().to_string())
            .collect()
    }

    /// A conta ao lado do nome de cada aba.
    fn tab_counts(seen: &Value) -> Vec<(String, String)> {
        seen["tabs"]
            .as_array()
            .expect("tabs")
            .iter()
            .map(|t| (t["anchor"].as_str().unwrap_or_default().to_string(), t["count"].as_str().unwrap_or_default().to_string()))
            .collect()
    }

    /// O `.md` baixado confere, cartão por cartão e campo por campo, contra
    /// `page` — a mesma tela que o passo `scrape` já leu —: cada aba vira um
    /// título, cada cartão (e as execuções e a resposta dentro dele) aparece
    /// pelo código ainda em negrito, e cada rótulo de campo como `- rótulo:`,
    /// sem tirar `**` nem crase do valor.
    fn assert_md_matches_the_whole_page(md: &str, page: &Value) {
        let labels: BTreeMap<String, String> = page["tabs"]
            .as_array()
            .expect("tabs")
            .iter()
            .map(|t| (t["anchor"].as_str().unwrap_or_default().to_string(), t["label"].as_str().unwrap_or_default().to_string()))
            .collect();
        fn each(item: &Value, md: &str) {
            let code = item["code"].as_str().unwrap_or_default();
            assert!(md.contains(&format!("**{code}**")), "{code} (still bold) missing from the .md:\n{md}");
            for field in item["fields"].as_array().expect("fields") {
                let label = field[0].as_str().unwrap_or_default();
                assert!(md.contains(&format!("- {label}: ")), "field {label:?} of {code} missing from the .md:\n{md}");
            }
            item["runs"].as_array().into_iter().flatten().for_each(|r| each(r, md));
            if !item["answer"].is_null() {
                each(&item["answer"], md);
            }
        }
        for section in page["sections"].as_array().expect("sections") {
            let heading = &labels[section["id"].as_str().unwrap_or_default()];
            assert!(md.contains(&format!("\n## {heading}\n")), "{heading:?} tab heading missing from the .md:\n{md}");
            section["items"].as_array().expect("items").iter().for_each(|i| each(i, md));
        }
    }

    /// O pedido que o detalhe da onda aberta mostra.
    fn prompt_of(seen: &Value) -> &Value {
        seen["detail"]["prompts"].as_array().and_then(|p| p.first()).unwrap_or_else(|| panic!("no prompt: {}", seen["detail"]))
    }

    /// Os dois templates leem o banco de dados da página. O da spec mostra os
    /// mesmos itens da página que o binário montava, agora nas abas do
    /// painel, com o estado de cada onda no gráfico e o pedido completo de
    /// cada uma no detalhe (o texto de cada item no lugar do código); a
    /// busca esconde o que não serve e as abas contam só o que casa; o botão
    /// baixa o `.md` com o mesmo conteúdo. O do projeto mostra as specs
    /// agrupadas por fase, com o link da página de cada uma.
    #[test]
    fn the_page_templates_read_the_database() {
        let lines = spec_lines();
        let steps = json!([
            {"do": "wait"}, {"do": "scrape", "as": "page"},
            {"do": "download", "as": "md"},
            {"do": "search", "value": "powershell"}, {"do": "scrape", "as": "search"},
            {"do": "search", "value": "WINDOWS"}, {"do": "scrape", "as": "windows"},
            {"do": "search", "value": "MSTD-DEC-0001"}, {"do": "scrape", "as": "code"},
            {"do": "search", "value": ""}, {"do": "hash", "value": "waves-4"}, {"do": "scrape", "as": "wave4"},
        ]);
        let got = run("spec", &spec_page_template(Locale::PtBr), Some(spec_database(&lines)), steps);
        let page = &got["page"];

        assert_eq!(page["state"], json!("ready"), "the page read the database");
        assert_eq!(page["statusHidden"], json!(true));
        assert_eq!((&page["title"], &page["phase"], &page["branch"]), (&json!("demo"), &json!("aprovada"), &json!("feature/demo → dev")));
        assert_eq!(page_seen(page), fixed_tabs(), "the same items as the fixed page, each in its tab");
        let runs: Vec<Value> = cards(page, "criteria")[0]["runs"].as_array().expect("runs").iter().map(card_seen).collect();
        assert_eq!(runs.len(), 1, "{runs:?}");
        assert_eq!((&runs[0]["code"], &runs[0]["status"]), (&json!("MSTD-CRUN-0001"), &json!("passou")), "the run inside its criterion");
        assert_eq!(page["goal"]["text"], json!("O Rust roda rápido: 3 a 14 ms por gancho. O custo está nas rodadas do modelo."));
        // A versão mais nova de cada item, sem o item retirado, com a marca do
        // que entrou depois da aprovação; a versão antiga fica na conversa. O
        // item retirado só aparece na aba dos removidos.
        let removed: Vec<&Value> = cards(page, "removed").iter().map(|i| &i["code"]).collect();
        assert!(removed.contains(&&json!("MSTD-NOTE-0001")), "{removed:?}");
        let elsewhere: Vec<String> = page["sections"]
            .as_array()
            .expect("sections")
            .iter()
            .filter(|s| s["id"] != json!("removed"))
            .flat_map(|s| s["items"].as_array().expect("items").iter())
            .map(|i| i["code"].as_str().unwrap_or_default().to_string())
            .collect();
        assert!(!elsewhere.contains(&"MSTD-NOTE-0001".to_string()), "the removed note is gone");
        let rule = cards(page, "agreed").iter().find(|i| i["code"] == json!("MSTD-RULE-0001")).expect("the rule");
        assert_eq!(rule["title"], json!("A trava de comandos confere o programa, as opções e o caminho."));
        assert_eq!(rule["mark"], json!("depois da aprovação"));
        let old = cards(page, "conversation").iter().find(|i| i["code"] == json!("MSTD-RULE-0001")).expect("the old version of the rule");
        assert_eq!((&old["status"], &old["anchored"]), (&json!("versão antiga"), &json!(false)));
        let tabs: Vec<&str> = page["tabs"].as_array().expect("tabs").iter().map(|t| t["anchor"].as_str().unwrap_or_default()).collect();
        assert_eq!(tabs, ["specification", "agreed", "criteria", "notes", "review", "progress", "conversation", "removed"]);
        let bars: Vec<Value> = page["chart"]["bars"].as_array().expect("bars").iter().map(|b| json!([b["wave"], b["class"]])).collect();
        assert_eq!(
            bars,
            [json!([1, "b wait"]), json!([2, "b done"]), json!([3, "b running on"]), json!([4, "b wait"])],
            "the wave states come from the database, and the running wave opens"
        );

        // O pedido completo, no detalhe da onda que roda: cada código vira o
        // texto da versão mais nova do item, com as linhas de lista dele.
        assert_eq!(page["detail"]["wave"], json!(3));
        let sent = prompt_of(page);
        let full = sent["text"].as_str().expect("the full prompt");
        for expected in [
            "MSTD-CTX-0001 — O Rust roda rápido: 3 a 14 ms por gancho.",
            "MSTD-CONC-0001 — Cerca de 11 arquivos de teste prendem frases da prosa atual.",
            "MSTD-RULE-0001 — A trava de comandos confere o programa, as opções e o caminho.",
            "vale para o PowerShell.",
            "MSTD-DEC-0001 — A página é publicada só nos marcos, e a MSTD-RULE-0001 continua valendo.",
            "MSTD-CRIT-0001 — Quando: O pedido montado de uma onda passa de 500 linhas.",
        ] {
            assert!(full.contains(expected), "{expected:?} is not in the full prompt:\n{full}");
        }
        assert!(!full.contains("`specification`: MSTD-CTX-0001"), "no line keeps only the codes:\n{full}");
        assert!(sent["html"].as_str().unwrap_or_default().contains("<li>vale para o PowerShell.</li>"), "{}", sent["html"]);
        assert_eq!(got["wave4"]["detail"]["wave"], json!(4), "the address opens wave 4");
        assert!(prompt_of(&got["wave4"])["text"].as_str().unwrap_or_default().contains("MSTD-RULE-0001 — A trava de comandos"));

        // A escolha do orquestrador gravada no envio, entre as medidas dele:
        // o item e a lição que saíram do pedido, e o item que entrou, cada um
        // com o motivo.
        let measures = page["detail"]["measures"].as_array().expect("measures");
        let analysis = measures.iter().find(|f| f[0] == json!("Análise antes do envio")).unwrap_or_else(|| panic!("{measures:?}"));
        assert_eq!(
            analysis[1],
            json!(
                "Tirou do pedido: MSTD-RULE-0001 (A regra fala da trava, não da tabela desta onda.); \
                 lição 12 (A lição é de outra onda.) · Pôs no pedido: MSTD-CTX-0001 (O contexto explica \
                 por que a tabela nasce vazia.)"
            ),
            "{measures:?}"
        );

        // O veredito final do agente de teste dedicado ganha a marca própria
        // na aba da revisão, mesmo apontando a mesma onda 2 do outro veredito.
        let review = cards(page, "review");
        assert_eq!(review.len(), 2, "{review:?}");
        assert_eq!((&review[0]["code"], &review[0]["extra"]), (&json!("MSTD-VERD-0001"), &json!([])));
        let final_item = &review[1];
        assert_eq!(
            (final_item["code"].clone(), final_item["title"].clone(), final_item["status"].clone(), final_item["extra"].clone()),
            (json!("MSTD-VERD-0002"), json!("As ondas se encaixam sem prova perdida."), json!("aprovada"), json!(["Veredito final"])),
            "{final_item}"
        );
        assert_eq!(card_seen(final_item)["fields"], json!([["Onda", "2"], ["Resultado", "aprovada"], ["Aceitação", "sim"]]));

        // O .md baixado tem o mesmo conteúdo, com o pedido completo.
        let md = got["md"]["data"].as_str().expect("the downloaded .md");
        assert_eq!(got["md"]["filename"], json!("demo.md"));
        for expected in [
            "# demo",
            "## Andamento",
            "## Conversa",
            "- **MSTD-RULE-0001** · depois da aprovação · 2026-09-12 12:02",
            "  - **MSTD-CTX-0001** — O Rust roda rápido: 3 a 14 ms por gancho.",
            "    - vale para o PowerShell.",
        ] {
            assert!(md.contains(expected), "{expected:?} is not in the .md:\n{md}");
        }
        assert!(!md.contains("`specification`: MSTD-CTX-0001"), "the .md shows the full prompt too");
        assert_md_matches_the_whole_page(md, page);

        // A busca, sem diferença de maiúscula, e pelo código; cada aba conta
        // só o que casa, e a aba sem nada diz isso.
        assert_eq!(visible_codes(&got["search"]), ["MSTD-RULE-0001"]);
        let count = |seen: &Value, tab: &str| tab_counts(seen).into_iter().find(|(a, _)| a == tab).map(|(_, c)| c).unwrap_or_default();
        assert_eq!((count(&got["search"], "agreed"), count(&got["search"], "notes")), ("1".to_string(), "0".to_string()));
        assert_eq!(panel(&got["search"], "notes")["empty"], json!("Nada nesta aba com essa busca."));
        assert_eq!(visible_codes(&got["windows"]), ["MSTD-REQ-0001", "MSTD-DEFER-0001"]);
        assert_eq!(count(&got["windows"], "notes"), "2");
        // A decisão vigente e, na conversa, a versão antiga dela.
        assert_eq!(visible_codes(&got["code"]), ["MSTD-DEC-0001", "MSTD-DEC-0001"]);
        assert_eq!(page["download"], json!("Baixar .md"));
        assert_eq!(page["search"], json!("Buscar texto ou código"));

        // A página do projeto: uma linha por spec, agrupadas por fase.
        let rows = [
            project_row("busca", Some("running"), Some("https://claude.ai/code/artifact/busca")),
            project_row("trava", Some("plan"), None),
            project_row("velha", None, None),
        ];
        let db = json!({"specs": rows.iter().map(|r| json!({"id": r.name, "data": {
            "name": r.name, "goal": r.goal, "phase": r.phase, "branch": r.branch,
            "created": r.created, "updated": r.updated, "url": r.url,
        }})).chain([json!({"id": "sem-nome", "data": {"goal": "Sem nome."}})]).collect::<Vec<_>>()});
        let got = run("project", &project_page_template(Locale::PtBr), Some(db), json!([{"do": "wait"}, {"do": "scrape", "as": "page"}]));
        let page = &got["page"];
        assert_eq!(page["state"], json!("ready"));
        let fixed: Value = serde_json::from_str(PROJECT_PAGE_FIXTURE).expect("the project page fixture is valid JSON");
        assert_eq!(page["stages"], fixed["stages"], "the same stages line as the fixed page");
        let expected = fixed["groups"].clone();
        let seen: Vec<Value> = page["groups"]
            .as_array()
            .expect("groups")
            .iter()
            .map(|g| {
                let rows: Vec<Value> = g["rows"].as_array().expect("rows").iter().map(|r| json!({"code": r["code"], "title": r["title"],
                    "status": r["status"], "date": r["date"], "fields": r["fields"].as_array().expect("fields").iter().map(|f| json!([f[0], f[1]])).collect::<Vec<_>>()})).collect();
                json!({"id": g["id"], "title": g["title"], "rows": rows})
            })
            .collect();
        assert_eq!(seen, expected.as_array().cloned().unwrap_or_default(), "the same groups and rows as the fixed page");
        let link = &page["groups"][1]["rows"][0]["fields"][4];
        assert_eq!(link[2], json!(r#"<a href="https://claude.ai/code/artifact/busca" target="_blank" rel="noopener">busca</a>"#));
    }

    /// A onda no detalhe dela: nada se repete na mesma tela, a hierarquia de
    /// títulos pára em três níveis, e o detalhe mostra o pedido inteiro que
    /// a onda recebeu (o molde e o texto, nessa ordem, uma vez cada), com as
    /// medidas do envio e o consumo dela. A linha do gasto que o binário
    /// compôs vai para o `.md`, sem ser montada de novo do lado do cliente.
    #[test]
    fn a_wave_shows_its_whole_request_once_and_in_order() {
        let lines = vec![
            json!({"v":1,"id":1,"at":"2026-09-19T09:00:00-03:00","type":"wave","author":"assistant",
                "n":9,"text":"A onda nove.","criteria":[1],"done_when":"A suíte passa.","origin":1}),
            json!({"v":1,"id":2,"at":"2026-09-19T09:01:00-03:00","type":"send","author":"binary","wave":9,
                "role":"wave","template":"# molde\n\nTexto do molde.","text":"# pedido\n\nTexto do pedido.",
                "lines":2,"chars":40,"items":[1],"mustard":"0.2.1",
                "model":"sonnet","model_used":"sonnet","steps":12,"tokens":3400,"origin":1}),
            json!({"v":1,"id":3,"at":"2026-09-19T09:02:00-03:00","type":"verdict","author":"review",
                "wave":9,"result":"approved","final":false,"text":"A onda fecha certo.","origin":1}),
        ];
        let steps = json!([{"do": "wait"}, {"do": "scrape", "as": "page"}, {"do": "download", "as": "md"}]);
        let mut db = spec_database(&lines);
        let line = "Gasto total: 3400 tokens de onda + 0 tokens de quem despachou = 3400 tokens.";
        db["computed"][0]["data"]["spend"] = json!(line);
        db["computed"][0]["data"]["tokens"] = json!({"waves": 3400, "caller": 0, "total": 3400, "turns": 12});
        db["computed"][0]["data"]["waves"] = json!({"9": "approved"});
        db["computed"][0]["data"]["prompts"] = json!({});
        let got = run("spec", &spec_page_template(Locale::PtBr), Some(db), steps);
        let page = &got["page"];

        let headings: Vec<&str> = page["headings"].as_array().expect("headings").iter().map(|h| h.as_str().unwrap_or_default()).collect();
        assert!(!headings.is_empty(), "the page has headings");
        assert!(headings.iter().all(|h| ["H1", "H2", "H3"].contains(h)), "only three heading levels: {headings:?}");

        let detail = &page["detail"];
        assert_eq!(detail["wave"], json!(9), "the only wave opens: {detail}");
        let labels: Vec<Value> = detail["measures"].as_array().expect("measures").iter().map(|f| f[0].clone()).collect();
        for key in ["page.field.model", "page.field.model_used", "page.field.steps", "page.field.tokens", "page.metrics.col.delivery"] {
            let label = json!(translate(key, Locale::PtBr));
            assert!(labels.contains(&label), "{key} ({label}) is not shown among {labels:?}");
        }
        let prompts = detail["prompts"].as_array().expect("prompts");
        let summaries: Vec<&str> = prompts.iter().map(|p| p["summary"].as_str().unwrap_or_default()).collect();
        assert_eq!(prompts.len(), 2, "the template and the text, once each: {summaries:?}");
        assert!(summaries[0].contains("Molde recebido") && summaries[1].contains("Pedido enviado"), "{summaries:?}");
        assert_eq!(prompts[0]["owner"], prompts[1]["owner"], "both belong to the same send");
        assert!(prompts.iter().all(|p| p["open"] == json!(false)), "the request starts folded");
        assert!(detail["meta"].as_str().unwrap_or_default().contains("3 mil tokens"), "{detail}");

        // A revisão da onda fica na aba dela, uma vez.
        let review = cards(page, "review");
        assert_eq!(review.len(), 1, "{review:?}");
        assert_eq!(review[0]["title"], json!("A onda fecha certo."));
        // O gasto que o binário compôs vai para o .md como veio.
        let md = got["md"]["data"].as_str().unwrap_or_default();
        assert!(md.contains(line), "{md}");
    }

    /// O ponto do levantamento respondido ganha a marca de fechado, e a
    /// resposta vai dentro do cartão dele, junto do ponto.
    #[test]
    fn an_answered_question_shows_as_closed() {
        let lines = vec![
            json!({"v":1,"id":1,"at":"2026-09-19T09:00:00-03:00","type":"point","author":"assistant",
                "block":"limits","gap":"Tamanho do pedido de cada onda","from":"gap","status":"open","origin":1,
                "facts":[{"text":"A montagem do pedido não tem teto de tamanho.","source":"apps/rt/src/commands/agent/render/mod.rs:798"}]}),
            json!({"v":1,"id":2,"at":"2026-09-19T09:01:00-03:00","type":"point","author":"assistant",
                "block":"limits","gap":"g","from":"gap","status":"closed","closes":1,"result":[1],"origin":1}),
        ];
        let steps = json!([{"do": "wait"}, {"do": "scrape", "as": "page"}]);
        let got = run("spec", &spec_page_template(Locale::PtBr), Some(spec_database(&lines)), steps);
        let items = cards(&got["page"], "agreed");
        assert_eq!(items.len(), 1, "the open point, with the one that closes it inside: {items:?}");
        let open_point = &items[0];
        assert!(open_point["title"].as_str().unwrap_or_default().contains("Tamanho do pedido de cada onda"), "{open_point}");
        assert_eq!(
            open_point["status"],
            json!(translate("page.value.closed", Locale::PtBr)),
            "an answered point shows the closed mark, not the open one: {open_point}"
        );
        assert_eq!(open_point["answer"]["code"], json!("MSTD-POINT-0002"), "{open_point}");
    }

    /// O botão de baixar o `.md` só aparece com o salvar arquivo do
    /// claude.ai (`downloads`) disponível: sem ele, baixar pelo navegador
    /// direto não funciona dentro do claude.ai, então o botão some em vez de
    /// tentar; com a capacidade, o clique salva o `.md`. A página declara a
    /// capacidade como `true`, o que o contrato da plataforma pede.
    #[test]
    fn the_download_button_hides_without_the_save_capability() {
        assert_eq!(
            serde_json::from_str::<Value>(SPEC_CAPABILITIES).unwrap()["downloads"],
            json!(true),
            "{SPEC_CAPABILITIES}"
        );

        let lines = vec![json!({"v":1,"id":1,"at":"2026-09-19T09:00:00-03:00","type":"message","author":"user","text":"o objetivo","origin":1})];
        let steps = json!([{"do": "wait"}, {"do": "scrape", "as": "page"}, {"do": "download", "as": "md"}]);

        let without = run_with_downloads("spec", &spec_page_template(Locale::PtBr), Some(spec_database(&lines)), steps.clone(), false);
        assert_eq!(without["page"]["downloadHidden"], json!(true), "sem a capacidade, o botão some: {without}");
        assert_eq!(without["md"], Value::Null, "sem a capacidade, o clique não salva nada: {without}");

        let with = run_with_downloads("spec", &spec_page_template(Locale::PtBr), Some(spec_database(&lines)), steps, true);
        assert_eq!(with["page"]["downloadHidden"], json!(false), "com a capacidade, o botão aparece: {with}");
        assert_eq!(with["md"]["filename"], json!("demo.md"), "{with}");
    }

    /// Na última aba, Removidos, cada item que saiu: o removido com o
    /// código, o texto, quem o removeu, quando e por quê; o expurgado por
    /// segredo só com a marca no lugar do texto. Vale para a tela e para o
    /// `.md` baixado. O item removido com as duas versões aparece uma vez,
    /// pela mais nova, e o item expurgado pelo formato de hoje continua à
    /// mostra na aba dele, com o resto do texto.
    #[test]
    fn the_removed_section_lists_what_left() {
        let mut lines = spec_lines();
        lines.extend([
            // O expurgo de hoje: o trecho já virou a marca na própria linha.
            json!({"v":1,"id":50,"at":"2026-09-12T14:00:00-03:00","type":"note","author":"assistant","text":"A chave … fica no cofre do time.","keys":["chave"],"origin":2}),
            json!({"v":1,"id":51,"at":"2026-09-12T14:01:00-03:00","type":"purge","author":"user","targets":[50],"reason":"secret","origin":2}),
            // A regra revista sai inteira: as duas versões.
            json!({"v":1,"id":52,"at":"2026-09-12T14:02:00-03:00","type":"remove","author":"user","targets":[9, 42],"reason":"A trava mudou de lugar.","origin":2}),
        ]);
        let content = lines.iter().map(Value::to_string).collect::<Vec<_>>().join("\n");
        let codes = parse_log(&content).codes();
        let code = |id: u64| codes.get(&id).cloned().unwrap_or_else(|| panic!("no code for {id}"));
        let steps = json!([{"do": "wait"}, {"do": "scrape", "as": "page"}, {"do": "download", "as": "md"}]);
        let got = run("spec", &spec_page_template(Locale::PtBr), Some(spec_database(&lines)), steps);
        let page = &got["page"];

        // A aba é a última da página.
        let last = page["tabs"].as_array().and_then(|t| t.last()).expect("a last tab");
        assert_eq!((&last["anchor"], &last["label"]), (&json!("removed"), &json!("Removidos")));
        let entries: Vec<Value> = cards(page, "removed")
            .iter()
            .map(|i| {
                let fields: Vec<Value> =
                    i["fields"].as_array().expect("fields").iter().map(|f| json!([f[0], f[1]])).collect();
                json!({"code": i["code"], "who": i["who"], "mark": i["mark"], "title": i["title"], "text": i["text"], "fields": fields})
            })
            .collect();
        assert_eq!(
            entries,
            [
                // O texto aberto só mostra o que vem depois do primeiro parágrafo: quando
                // é só um parágrafo (ou a marca do expurgo), ele já é o título, e o corpo
                // aberto fica vazio em vez de repeti-lo.
                json!({"code": code(34), "who": "anotação", "mark": "removido",
                    "title": "Anotação colada por engano.", "text": "",
                    "fields": [["Removido por", "assistente"], ["Removido em", "2026-09-12 11:10"],
                        ["Motivo", "Colada por engano."], ["Registro", code(36)]]}),
                json!({"code": code(37), "who": "mensagem", "mark": "expurgado", "title": "…", "text": "",
                    "fields": [["Expurgado por", "assistente"], ["Expurgado em", "2026-09-12 11:12"],
                        ["Motivo", "segredo"], ["Registro", code(38)]]}),
                json!({"code": code(42), "who": "regra", "mark": "removido",
                    "title": "A trava de comandos confere o programa, as opções e o caminho.",
                    "text": "vale para o Bash;vale para o PowerShell.",
                    "fields": [["Removido por", "usuário"], ["Removido em", "2026-09-12 14:02"],
                        ["Motivo", "A trava mudou de lugar."], ["Registro", code(52)]]}),
                json!({"code": code(50), "who": "anotação", "mark": "expurgado", "title": "…", "text": "",
                    "fields": [["Expurgado por", "usuário"], ["Expurgado em", "2026-09-12 14:01"],
                        ["Motivo", "segredo"], ["Registro", code(51)]]}),
            ],
            "each item that left, the purged ones only with the mark"
        );
        assert_eq!(code(9), code(42), "the two versions of the rule are one item");
        // O expurgo de hoje não tira o item da leitura: ele segue nas
        // anotações com o resto do texto, e a aba dos removidos não o repete.
        let kept = cards(page, "notes").iter().find(|i| i["code"] == json!(code(50)));
        assert_eq!(kept.map(|i| i["title"].clone()), Some(json!("A chave … fica no cofre do time.")));
        assert!(!panel(page, "removed").to_string().contains("cofre"), "the purged text stays out of the removed tab");

        // O .md baixado tem a mesma seção, no fim.
        let md = got["md"]["data"].as_str().expect("the downloaded .md");
        let (before, removed) = md.split_once("\n## Removidos\n").unwrap_or_else(|| panic!("no removed section:\n{md}"));
        assert!(before.contains("\n## Conversa\n") && !removed.contains("\n## "), "the removed section is the last:\n{md}");
        for expected in [
            "- **MSTD-NOTE-0001** · anotação · removido · 2026-09-12 11:08 — Anotação colada por engano.",
            "  - Removido por: assistente",
            "  - Removido em: 2026-09-12 11:10",
            "  - Motivo: Colada por engano.",
            &format!("  - Registro: {}", code(36)),
            &format!("- **{}** · mensagem · expurgado · 2026-09-12 11:11 — …", code(37)),
            "  - Expurgado em: 2026-09-12 11:12",
            &format!("- **{}** · anotação · expurgado · 2026-09-12 14:00 — …", code(50)),
            "  - Expurgado por: usuário",
            "  - Motivo: segredo",
        ] {
            assert!(removed.contains(expected), "{expected:?} is not in the removed section of the .md:\n{removed}");
        }
        assert!(!removed.contains("cofre"), "the purged text stays out of the .md's removed section:\n{removed}");
        assert_eq!(removed.matches(&format!("**{}**", code(42))).count(), 1, "the removed rule shows once:\n{removed}");
    }

    /// Por decisão da onda 13, o item que continua à mostra numa versão nova
    /// não entra em Removidos quando só a versão antiga dele foi removida: a
    /// regra revista some da conversa, mas a regra em si segue de pé pela
    /// versão nova, com o mesmo código.
    #[test]
    fn the_removed_section_handles_an_item_still_shown() {
        let lines = vec![
            json!({"v":1,"id":1,"at":"2026-09-19T09:00:00-03:00","type":"rule","author":"assistant",
                "text":"A trava confere o programa.","keys":["trava"],"example":"`rm -rf`.","origin":1}),
            json!({"v":1,"id":2,"at":"2026-09-19T09:01:00-03:00","type":"rule","author":"assistant",
                "text":"A trava confere o programa e as opções.","keys":["trava"],"example":"`rm -rf pasta`.",
                "origin":1,"replaces":1}),
            json!({"v":1,"id":3,"at":"2026-09-19T09:02:00-03:00","type":"remove","author":"user","targets":[1],
                "reason":"A versão antiga saiu.","origin":1}),
        ];
        let steps = json!([{"do": "wait"}, {"do": "scrape", "as": "page"}]);
        let got = run("spec", &spec_page_template(Locale::PtBr), Some(spec_database(&lines)), steps);
        assert_eq!(cards(&got["page"], "removed").len(), 0, "the rule stays shown through the newer version");
        assert_eq!(cards(&got["page"], "agreed").len(), 1);
    }

    /// O expurgo do formato antigo, cujo item nunca chegou ao banco da página
    /// (a linha dele no `spec.ndjson` já nasceu esvaziada), aparece em
    /// Removidos com o número no lugar do código: sem o item no banco, não há
    /// como montar o código dele.
    #[test]
    fn an_old_format_purge_shows_the_number_in_place_of_the_code() {
        let lines = vec![json!({"v":1,"id":2,"at":"2026-09-19T09:01:00-03:00","type":"purge","author":"user",
            "targets":[1],"reason":"secret","origin":1})];
        let steps = json!([{"do": "wait"}, {"do": "scrape", "as": "page"}]);
        let got = run("spec", &spec_page_template(Locale::PtBr), Some(spec_database(&lines)), steps);
        let entries = cards(&got["page"], "removed");
        assert_eq!(entries.len(), 1, "{entries:?}");
        assert_eq!(
            entries[0]["code"],
            json!("1"),
            "an item that never reached the database shows its number in place of the code: {}",
            entries[0]
        );
        assert_eq!(entries[0]["mark"], json!(translate("page.removed.purged", Locale::PtBr)));
    }

    fn project_row(name: &str, phase: Option<&str>, url: Option<&str>) -> ProjectRow {
        ProjectRow {
            name: name.to_string(),
            created: Some("2026-09-01T10:00:00-03:00".to_string()),
            updated: Some("2026-09-17T10:00:00-03:00".to_string()),
            phase: phase.map(str::to_string),
            branch: Some(format!("feature/{name}")),
            goal: Some(format!("Objetivo de {name}.")),
            url: url.map(str::to_string),
            titles: Vec::new(),
        }
    }

    /// Sem banco, e com o banco ainda vazio, as duas páginas abrem e dizem que
    /// ainda não há dados.
    #[test]
    fn without_the_database_the_pages_say_there_is_no_data_yet() {
        let steps = json!([{"do": "wait"}, {"do": "scrape", "as": "page"}]);
        let no_data = translate("page.no_data", Locale::PtBr);
        for (page, html) in [("spec", spec_page_template(Locale::PtBr)), ("project", project_page_template(Locale::PtBr))] {
            for db in [None, Some(json!({}))] {
                let got = run(page, &html, db.clone(), steps.clone());
                assert_eq!(got["page"]["state"], json!("empty"), "{page} {db:?}");
                assert_eq!(got["page"]["status"], json!(no_data), "{page} {db:?}");
                assert_eq!(got["page"]["statusHidden"], json!(false), "{page} {db:?}");
            }
        }
    }

    /// O documento calculado já tem dado — uma cópia já rodou —, mas a
    /// coleção de faixas está vazia: o modelo lido não é o que a cópia de
    /// agora escreve, e a página diz que o modelo está velho, com o comando
    /// que o atualiza, em vez da linha de banco vazio.
    #[test]
    fn a_populated_computed_doc_with_no_range_items_names_the_stale_template() {
        let steps = json!([{"do": "wait"}, {"do": "scrape", "as": "page"}]);
        let stale = translate("page.stale_template", Locale::PtBr);
        let db = json!({"computed": [{"id": "current", "data": {"spec": "demo", "last": 9}}]});
        let got = run("spec", &spec_page_template(Locale::PtBr), Some(db), steps);
        assert_eq!(got["page"]["state"], json!("stale"), "{got}");
        assert_eq!(got["page"]["status"], json!(stale), "{got}");
        assert_eq!(got["page"]["statusHidden"], json!(false), "{got}");
    }

    /// Sem a cópia ter rodado ainda — o documento calculado vazio —, a
    /// coleção de faixas vazia continua a leitura genuína de uma spec nova: a
    /// linha é a de banco vazio, não a de modelo velho, mesmo com o
    /// documento calculado presente e vazio.
    #[test]
    fn an_empty_computed_doc_with_no_range_items_still_says_no_data() {
        let steps = json!([{"do": "wait"}, {"do": "scrape", "as": "page"}]);
        let no_data = translate("page.no_data", Locale::PtBr);
        let db = json!({"computed": [{"id": "current", "data": {}}]});
        let got = run("spec", &spec_page_template(Locale::PtBr), Some(db), steps);
        assert_eq!(got["page"]["state"], json!("empty"), "{got}");
        assert_eq!(got["page"]["status"], json!(no_data), "{got}");
    }

    /// Uma cópia nova chega com a página aberta: o item novo aparece, o item
    /// que saiu do banco some e o estado novo da onda vale no gráfico, sem
    /// recarregar; a onda aberta continua aberta.
    /// A faixa tocada vai inteira, com o item novo dentro e o que saiu fora
    /// — não um `set` e um `delete` avulsos.
    #[test]
    fn a_new_copy_updates_the_open_page() {
        let lines = spec_lines();
        let note = json!({"v":1,"id":45,"at":"2026-09-12T13:00:00-03:00","type":"note","author":"assistant","code":"MSTD-NOTE-0009","text":"Nota que chegou depois.","keys":["nota"],"origin":2});
        let mut range0: Vec<Value> = lines.iter().filter(|l| l["id"].as_u64() != Some(35)).cloned().collect();
        range0.push(note.clone());
        range0.sort_by_key(|l| l["id"].as_u64().unwrap_or(0));
        let steps = json!([
            {"do": "wait"}, {"do": "scrape", "as": "before"},
            {"do": "copy", "set": {"ranges": [{"id": "0", "data": {"seq": 0, "items": range0}}],
                "computed": [{"id": "current", "data": {"spec": "demo", "waves": {"2": "approved", "3": "delivered"}}}]}},
            {"do": "scrape", "as": "after"},
        ]);
        let got = run("spec", &spec_page_template(Locale::PtBr), Some(spec_database(&lines)), steps);
        let notes = |seen: &Value| -> Vec<String> {
            cards(seen, "notes").iter().map(|i| i["title"].as_str().unwrap_or_default().to_string()).collect()
        };
        assert!(notes(&got["before"]).iter().any(|t| t.starts_with("O pull request 276")));
        let mut after = notes(&got["after"]);
        after.retain(|t| !t.starts_with("A tarefa MSTD-TASK-0001"));
        assert_eq!(
            after,
            [
                "Incluir o Windows no teste de duas gravações ao mesmo tempo.",
                "Medir o antivírus do Windows na verificação automática.",
                "Nota que chegou depois.",
            ],
            "the new note came in and the deleted one left"
        );
        let bar3 = |seen: &Value| seen["chart"]["bars"].as_array().expect("bars").iter().find(|b| b["wave"] == json!(3)).expect("bar 3")["class"].clone();
        assert_eq!((bar3(&got["before"]), bar3(&got["after"])), (json!("b running on"), json!("b done on")));
    }

    /// Uma cópia que só troca a faixa do meio sem o item que saiu, como a
    /// saída de um item com trecho de segredo, não traz item novo nem muda
    /// onda, pedido ou rtk: sem o `last` do documento calculado mudando, nem
    /// ele nem o item de maior número mudam, e a página aberta fica velha.
    /// Com o `last` sempre mudando a cada cópia, a escuta relê a página.
    /// Quando uma escuta falha, a página mostra um aviso curto sem perder o
    /// que já tinha, e a próxima cópia que der certo tira o aviso.
    #[test]
    fn the_open_page_reloads_when_a_middle_item_leaves() {
        let lines = spec_lines();
        let without_35: Vec<Value> = lines.iter().filter(|l| l["id"].as_u64() != Some(35)).cloned().collect();
        let waves = json!({"2": "approved", "3": "running"});
        let computed_with_last = |last: u64| json!({"spec": "demo", "last": last, "waves": waves, "prompts": {}, "rtk": []});
        let db = json!({"ranges": range_docs(&lines), "computed": [{"id": "current", "data": computed_with_last(44)}]});
        let steps = json!([
            {"do": "wait"}, {"do": "scrape", "as": "before"},
            {"do": "copy", "set": {"computed": [{"id": "current", "data": computed_with_last(53)}],
                "ranges": [{"id": "0", "data": {"seq": 0, "items": without_35}}]}},
            {"do": "scrape", "as": "after"},
            {"do": "fail", "path": "computed/current"},
            {"do": "scrape", "as": "after_fail"},
            {"do": "copy", "set": {"computed": [{"id": "current", "data": computed_with_last(60)}]}, "delete": {}},
            {"do": "scrape", "as": "recovered"},
        ]);
        let got = run("spec", &spec_page_template(Locale::PtBr), Some(db), steps);
        let notes = |seen: &Value| -> Vec<String> {
            cards(seen, "notes").iter().map(|i| i["title"].as_str().unwrap_or_default().to_string()).collect()
        };
        assert!(notes(&got["before"]).iter().any(|t| t.starts_with("O pull request 276")), "the note is there before the copy");
        assert!(
            !notes(&got["after"]).iter().any(|t| t.starts_with("O pull request 276")),
            "a copy that only removes a middle item still changes `last`, so the open page reloads: {:?}",
            notes(&got["after"])
        );
        assert_eq!(got["after"]["statusHidden"], json!(true), "the page shows normally after the reload");

        assert_eq!(got["after_fail"]["status"], json!(translate("page.watch_failed", Locale::PtBr)));
        assert_eq!(got["after_fail"]["statusHidden"], json!(false), "a failed listener shows the warning: {}", got["after_fail"]["status"]);
        assert_eq!(notes(&got["after_fail"]), notes(&got["after"]), "the failed listener does not lose what the page already had");

        assert_eq!(got["recovered"]["statusHidden"], json!(true), "the next copy that succeeds clears the warning");
    }

    /// Uma cópia que só muda o documento das coisas calculadas — nenhum item
    /// novo, nenhum apagado, nenhum tocado — sozinha faz a página aberta
    /// reler: a escuta de `computed/current` não depende de nada acontecer
    /// na coleção dos itens. As outras provas de recarga sempre mudavam as
    /// duas coisas juntas (um item novo ou apagado ao lado da mudança no
    /// documento calculado), então cortar só a escuta do documento calculado
    /// não derrubava nenhuma delas — a prova isolada que a revisão de 18/09
    /// pediu (onda 7).
    #[test]
    fn a_change_only_in_the_computed_document_alone_reloads_the_page() {
        let lines = spec_lines();
        let db = spec_database(&lines);
        let mut moved = db.clone();
        moved["computed"][0]["data"]["waves"]["3"] = json!("approved");
        let steps = json!([
            {"do": "wait"}, {"do": "scrape", "as": "before"},
            {"do": "copy", "set": {"computed": moved["computed"].clone()}},
            {"do": "scrape", "as": "after"},
        ]);
        // `run` já derruba o teste se a escuta não recarregar: o passo
        // `copy` espera `data-renders` crescer e, sem isso, o harness
        // registra o erro que `run` confere.
        let got = run("spec", &spec_page_template(Locale::PtBr), Some(db), steps);
        let bars_of = |seen: &Value| seen["chart"]["bars"].clone();
        assert_ne!(bars_of(&got["before"]), bars_of(&got["after"]), "the reload read the new wave state: {got}");
    }

    /// Uma spec longa é lida inteira, em páginas de até 500 documentos da
    /// coleção das faixas, seguindo o cursor da mais velha para a mais nova:
    /// aqui, cada item na própria faixa (só para este teste — a faixa de
    /// verdade tem [`RANGE_WIDTH`] itens, e só passa de 500 documentos com
    /// mais de 50 mil itens), 1.200 itens somados aos da spec de exemplo
    /// pedem três idas ao banco. A aba longa mostra 40 cartões e o botão de
    /// mostrar mais, que traz os 40 seguintes.
    #[test]
    fn a_long_spec_is_read_in_pages() {
        let mut lines = spec_lines();
        let first = 100;
        lines.extend((0..1_200).map(|i| {
            json!({"v":1,"id":first + i,"at":"2026-09-13T10:00:00-03:00","type":"note","author":"assistant","text":format!("Nota {i}."),"keys":["k"],"origin":2})
        }));
        // Cada item na própria faixa (comentário do teste acima), mas `seq`
        // segue o formato real: início da faixa vezes mil mais o número do
        // pedaço. Aqui o item é o único pedaço da própria faixa, então o
        // número do pedaço é sempre 0 e `seq` é o id vezes mil; sem `chunks`,
        // a leitura assume 1 pedaço, o que já é o caso.
        let ranges: Vec<Value> = lines
            .iter()
            .map(|l| {
                let id = l["id"].as_u64().unwrap_or(0);
                json!({"id": id.to_string(), "data": {"seq": id * 1_000, "items": [l.clone()]}})
            })
            .collect();
        let waves: serde_json::Map<String, Value> =
            wave_states().into_iter().map(|(n, s)| (n.to_string(), json!(state_name(s)))).collect();
        let db = json!({
            "ranges": ranges,
            "computed": [{"id": "current", "data": {"spec": "demo", "waves": waves, "prompts": {}, "rtk": []}}],
        });
        let steps = json!([{"do": "wait"}, {"do": "scrape", "as": "page"}, {"do": "reads", "as": "reads"},
            {"do": "more", "value": "notes"}, {"do": "scrape", "as": "more"}]);
        let got = run("spec", &spec_page_template(Locale::PtBr), Some(db), steps);
        let notes = cards(&got["page"], "notes");
        assert_eq!(notes.len(), 1_204, "every note of the long spec, with the plan finding");
        let shown = |seen: &Value| cards(seen, "notes").iter().filter(|i| i["hidden"] == json!(false)).count();
        assert_eq!(tab_counts(&got["page"])[3], ("notes".to_string(), "1204".to_string()));
        assert_eq!((shown(&got["page"]), panel(&got["page"], "notes")["more"].clone()), (40, json!("Mostrar mais 40 de 1164")));
        assert_eq!((shown(&got["more"]), panel(&got["more"], "notes")["more"].clone()), (80, json!("Mostrar mais 40 de 1124")));
        let pages: Vec<(Value, Value)> = got["reads"]
            .as_array()
            .expect("reads")
            .iter()
            .filter(|r| r["path"] == json!(RANGES))
            .map(|r| (r["filters"][0][2].clone(), r["size"].clone()))
            .collect();
        let last_of_first_page = lines.iter().map(|l| l["id"].as_u64().unwrap_or(0) * 1_000).collect::<Vec<_>>()[499];
        assert_eq!(pages.len(), 3, "{pages:?}");
        assert_eq!(pages[0], (json!(-1), json!(500)));
        assert_eq!(pages[1], (json!(last_of_first_page), json!(500)));
        assert_eq!(pages[2].1, json!(lines.len() - 1_000));
    }

    /// O primeiro pedaço de uma faixa leva `chunks`, quantos pedaços ela tem
    /// agora. Um pedaço velho, que sobrou de antes de a faixa encolher — um
    /// item que um expurgo tirou de vez do jeito novo da faixa, por exemplo
    /// — fica no banco sem ninguém apagar; a leitura só lê os pedaços que a
    /// contagem do primeiro cobre, e o item do pedaço que sobrou não volta a
    /// aparecer na página. Sem a contagem, ele reaparece: a página não tem
    /// nenhum outro jeito de saber que aquele pedaço já não vale.
    #[test]
    fn a_stale_chunk_left_behind_by_a_shrunk_range_is_not_read_twice() {
        let lines = spec_lines();
        let mut db = spec_database(&lines);
        let ranges = db["ranges"].as_array_mut().expect("ranges");
        assert_eq!(ranges.len(), 1, "the fixture fits one range: {ranges:?}");
        ranges[0]["data"]["chunks"] = json!(1);
        // Um item que só existe no pedaço velho — nenhum item de hoje tem
        // este número —, do jeito que um item expurgado de vez ficaria: o
        // pedaço novo da faixa já não o leva.
        let stale_item = json!({"v": 1, "id": 90, "at": "2026-09-12T11:10:00-03:00", "type": "note",
            "author": "assistant", "code": "MSTD-NOTE-0099", "text": "Anotação que devia ter saído da faixa.",
            "keys": ["k"], "origin": 2});
        ranges.push(json!({"id": "0-2", "data": {"seq": 1, "items": [stale_item]}}));

        let steps = json!([{"do": "wait"}, {"do": "scrape", "as": "page"}]);
        let baseline =
            run("spec", &spec_page_template(Locale::PtBr), Some(spec_database(&lines)), steps.clone());
        let with_stale = run("spec", &spec_page_template(Locale::PtBr), Some(db), steps);
        assert_eq!(with_stale["page"]["state"], json!("ready"), "the page read the database");
        assert_eq!(
            page_seen(&with_stale["page"]),
            page_seen(&baseline["page"]),
            "the item left behind in the stale chunk does not come back: {}",
            visible_codes(&with_stale["page"]).join(", "),
        );
    }

    /// Um evento de tarefa para os testes da lista Agora: com onda ou no
    /// backlog, com ou sem título curto.
    fn board_task(id: u64, code: &str, wave: Option<u64>, title: Option<&str>, text: &str, depends_on: Value) -> Value {
        let mut e = json!({"v":1,"id":id,"code":code,"at":"2026-09-22T10:00:00-03:00","type":"task","author":"assistant",
            "text":text,"files":[],"depends_on":depends_on,"origin":1});
        if let Some(n) = wave {
            e["wave"] = json!(n);
        }
        if let Some(title) = title {
            e["title"] = json!(title);
        }
        e
    }

    /// Um evento de onda, como a rodada o grava.
    fn board_wave(id: u64, n: u64) -> Value {
        json!({"v":1,"id":id,"at":"2026-09-22T09:00:00-03:00","type":"wave","author":"binary","n":n,
            "text":format!("O lote {n}."),"criteria":[],"done_when":"A suíte passa.","origin":1})
    }

    /// Um evento de estado da spec, na fase `phase`.
    fn board_state(id: u64, phase: &str) -> Value {
        json!({"v":1,"id":id,"at":"2026-09-22T08:00:00-03:00","type":"state","author":"binary","phase":phase})
    }

    /// Abre a página da spec com `lines` e o estado calculado das ondas, e
    /// diz o que ela mostra (e o `.md` baixado, com `download`).
    fn open_board(lines: &[Value], waves: Value, lang: Locale, download: bool) -> Value {
        let db = json!({"ranges": range_docs(lines),
            "computed": [{"id": "current", "data": {"spec": "demo", "waves": waves, "prompts": {}, "rtk": []}}]});
        let mut steps = vec![json!({"do": "wait"}), json!({"do": "scrape", "as": "page"})];
        if download {
            steps.push(json!({"do": "download", "as": "md"}));
        }
        run("spec", &spec_page_template(lang), Some(db), json!(steps))
    }

    /// As linhas da lista Agora, na ordem: `[tipo, selo, título, lado direito]`
    /// para cada onda e tarefa, o nome de cada grupo e a linha do fim.
    fn now_rows(page: &Value) -> Vec<Value> {
        page["now"]["parts"]
            .as_array()
            .expect("parts")
            .iter()
            .map(|p| match p["kind"].as_str().unwrap_or_default() {
                "group" | "after" => json!([p["kind"], p["text"]]),
                _ => json!([p["kind"], p["pill"], p["title"], p["meta"]]),
            })
            .collect()
    }

    /// A lista Agora: a onda com várias tarefas diz quantas são e, aberta,
    /// os títulos numerados; a tarefa sem título aparece pela primeira
    /// frase, inteira até 90 caracteres e cortada a partir de 91. A tarefa
    /// do backlog que espera outra do backlog diz quantas espera; a
    /// dependência que já entregou não segura nada. A linha da tarefa do
    /// backlog tem o endereço dela, e abre no texto dela. Com o backlog
    /// vazio e nada rodando, a lista não tem grupo e faltam a revisão final
    /// e o fechamento; na spec fechada, nada falta.
    #[test]
    fn a_lista_agora_mostra_o_que_roda_e_o_backlog() {
        let s90 = format!("Noventa {}.", "n".repeat(81));
        let s91 = format!("Noventa e um {}.", "u".repeat(77));
        assert_eq!((s90.chars().count(), s91.chars().count()), (90, 91));
        let cut91 = format!("Noventa e um {}…", "u".repeat(76));
        let history = vec![
            board_wave(2, 1),
            board_task(3, "MSTD-TASK-0001", Some(1), Some("Base entregue"), "A base. Mais texto.", json!([])),
            board_wave(4, 2),
            board_task(5, "MSTD-TASK-0002", Some(2), Some("Segunda base"), "A segunda base.", json!([])),
            board_wave(6, 3),
            board_task(7, "MSTD-TASK-0003", Some(3), Some("A lista abre a página"), "Texto longo da tarefa. Mais.", json!([])),
            board_task(8, "MSTD-TASK-0004", Some(3), None, &format!("{s90} Depois."), json!([])),
            board_task(9, "MSTD-TASK-0005", Some(3), None, &format!("{s91} Depois."), json!([])),
        ];
        let backlog = vec![
            board_task(10, "MSTD-TASK-0010", None, Some("Título curto do backlog"), "A tarefa com título. Segunda frase.",
                json!(["MSTD-TASK-0011", 3])),
            board_task(11, "MSTD-TASK-0011", None, None, "A tarefa sem título espera nada. Segunda frase.", json!([3])),
        ];

        let running: Vec<Value> = [vec![board_state(1, "running")], history.clone(), backlog].concat();
        let got = open_board(&running, json!({"1": "approved", "2": "approved", "3": "running"}), Locale::PtBr, true);
        let page = &got["page"];
        assert_eq!(
            now_rows(page),
            vec![
                json!(["group", "Rodando"]),
                json!(["wave", "ONDA 3", "3 tarefas", "0 arquivos · em andamento"]),
                json!(["group", "Backlog · tarefas que ainda não viraram onda"]),
                json!(["backlog", "ESPERA", "Título curto do backlog", "0 arquivos · espera 1 tarefa do backlog"]),
                json!(["backlog", "PRONTA", "A tarefa sem título espera nada.", "0 arquivos"]),
                json!(["after", "Depois vêm a revisão final e o fechamento."]),
            ],
            "{}",
            page["now"]
        );
        let parts = page["now"]["parts"].as_array().expect("parts");
        assert_eq!(parts[1]["tasks"], json!(["A lista abre a página", s90, cut91]));
        assert_eq!((&parts[3]["id"], &parts[4]["id"]), (&json!("MSTD-TASK-0010"), &json!("MSTD-TASK-0011")));
        assert!(parts[4]["body"].as_str().unwrap_or_default().contains("Segunda frase."), "{}", parts[4]);
        // O .md baixado traz a mesma lista, antes das abas.
        let md = got["md"]["data"].as_str().unwrap_or_default();
        let now_at = md.find("\n## Agora\n").unwrap_or_else(|| panic!("no Agora in the .md:\n{md}"));
        assert!(now_at < md.find("\n## Especificação\n").unwrap_or(0), "{md}");
        for line in [
            "- **Onda 3** · em andamento · 0 arquivos — 3 tarefas",
            "  1. A lista abre a página",
            "### Backlog · tarefas que ainda não viraram onda",
            "- **MSTD-TASK-0010** · espera 1 tarefa do backlog · 2026-09-22 10:00 — A tarefa com título. Segunda frase.",
            "Depois vêm a revisão final e o fechamento.",
        ] {
            assert!(md.lines().any(|l| l == line), "{line:?} not in the .md:\n{md}");
        }

        // O backlog vazio e nenhuma onda rodando, numa spec que não fechou.
        let quiet: Vec<Value> = [vec![board_state(1, "running")], history.clone()].concat();
        let all_done = json!({"1": "approved", "2": "approved", "3": "approved"});
        let page = open_board(&quiet, all_done.clone(), Locale::PtBr, false)["page"].clone();
        assert_eq!(now_rows(&page), vec![json!(["after", "Faltam a revisão final e o fechamento."])]);

        // A spec fechada.
        let closed: Vec<Value> = [vec![board_state(1, "running")], history, vec![board_state(12, "closed")]].concat();
        let page = open_board(&closed, all_done, Locale::PtBr, false)["page"].clone();
        assert_eq!(now_rows(&page), vec![json!(["after", "Nada falta."])]);
    }

    /// Rodando mostra toda onda que não foi entregue nem aprovada, e não só a
    /// que roda: a onda que o backlog já formou e espera sair (sem estado
    /// calculado nenhum) e a reprovada que volta para conserto ganham cada
    /// uma a sua linha, com o selo, a situação e as tarefas. Enquanto uma
    /// delas existir, a lista nunca diz que faltam só a revisão final e o
    /// fechamento, nem com o backlog vazio e nenhuma onda rodando.
    #[test]
    fn a_lista_agora_mostra_a_onda_que_espera_e_a_reprovada() {
        let task = |id: u64, code: &str, wave: u64, title: &str| {
            board_task(id, code, Some(wave), Some(title), "O texto da tarefa.", json!([]))
        };
        let state = board_state(1, "running");
        let delivered = vec![board_wave(2, 1), task(3, "MSTD-TASK-0001", 1, "A base entregue")];
        let running = vec![board_wave(4, 2), task(5, "MSTD-TASK-0002", 2, "O lote que roda")];
        let waiting = vec![
            board_wave(6, 3),
            task(7, "MSTD-TASK-0003", 3, "O scan lê tudo"),
            task(8, "MSTD-TASK-0004", 3, "O mapa mostra o uso"),
        ];
        let rejected = vec![board_wave(9, 4), task(10, "MSTD-TASK-0005", 4, "O conserto do quadro")];
        let open = |lines: Vec<Value>, waves: Value, lang: Locale| open_board(&lines, waves, lang, false)["page"].clone();
        let pills = |page: &Value| -> Vec<Value> {
            page["now"]["parts"]
                .as_array()
                .expect("parts")
                .iter()
                .filter(|p| p["kind"] == json!("wave"))
                .map(|p| json!([p["pill"], p["pillClass"], p["title"], p["meta"], p["tasks"]]))
                .collect()
        };
        let tail = |page: &Value| now_rows(page).last().cloned().unwrap_or_default();

        let all: Vec<Value> = [vec![state.clone()], delivered.clone(), running, waiting.clone(), rejected.clone()].concat();
        let states = json!({"1": "approved", "2": "running", "4": "rejected"});
        let page = open(all.clone(), states.clone(), Locale::PtBr);
        assert_eq!(
            pills(&page),
            vec![
                json!(["ONDA 2", "pill running", "O lote que roda", "0 arquivos · em andamento", []]),
                json!(["ONDA 3", "pill wait", "2 tarefas", "0 arquivos · espera sair", ["O scan lê tudo", "O mapa mostra o uso"]]),
                json!(["ONDA 4", "pill fail", "O conserto do quadro", "0 arquivos · volta para conserto", []]),
            ],
            "{}",
            page["now"]
        );
        assert_eq!(tail(&page), json!(["after", "Depois vêm a revisão final e o fechamento."]));
        let page = open(all, states, Locale::EnUs);
        assert_eq!(
            pills(&page),
            vec![
                json!(["WAVE 2", "pill running", "O lote que roda", "0 files · in progress", []]),
                json!(["WAVE 3", "pill wait", "2 tasks", "0 files · waiting to go out", ["O scan lê tudo", "O mapa mostra o uso"]]),
                json!(["WAVE 4", "pill fail", "O conserto do quadro", "0 files · back for a fix", []]),
            ],
            "{}",
            page["now"]
        );
        assert_eq!(tail(&page), json!(["after", "Then come the final review and the closing."]));

        // Só a onda que espera sair, com o backlog vazio e nada rodando.
        let page = open([vec![state.clone()], delivered.clone(), waiting].concat(), json!({"1": "approved"}), Locale::PtBr);
        assert_eq!(pills(&page).len(), 1, "{}", page["now"]);
        assert_eq!(tail(&page), json!(["after", "Depois vêm a revisão final e o fechamento."]));

        // Só a onda reprovada, do mesmo jeito.
        let page = open([vec![state], delivered, rejected].concat(), json!({"1": "approved", "4": "rejected"}), Locale::PtBr);
        assert_eq!(pills(&page)[0][0], json!("ONDA 4"));
        assert_eq!(tail(&page), json!(["after", "Depois vêm a revisão final e o fechamento."]));
    }

    /// A spec do painel: o objetivo no primeiro contexto, uma onda entregue
    /// com três tarefas, o pedido e o commit dela, duas ondas em andamento,
    /// duas tarefas no backlog (uma que espera uma tarefa da onda 2 e uma
    /// pronta), três critérios (dois verdes e um sem prova), os envios com
    /// tokens e turnos, e ao menos um item de cada bloco. A palavra
    /// "girassol" só está numa regra e numa anotação.
    fn dashboard_lines() -> Vec<Value> {
        let at = |m: u64| format!("2026-09-22T{:02}:{:02}:00-03:00", 8 + m / 60, m % 60);
        let task = |id: u64, wave: Option<u64>, title: &str, depends_on: Value| {
            json!({"v":1,"id":id,"at":at(id),"type":"task","author":"assistant","title":title,
                "text":format!("O texto inteiro da tarefa {id}."),
                "files":[{"path":format!("src/t{id}.rs"),"new":true}],"covers":[20],"depends_on":depends_on,"origin":2,
                "wave":wave})
        };
        let mut lines = vec![
            json!({"v":1,"id":1,"at":at(1),"type":"state","author":"binary","phase":"running","branch":"feature/painel","base":"dev"}),
            json!({"v":1,"id":2,"at":at(2),"type":"context","author":"assistant","text":"O painel diz cada coisa uma vez.\n\nO segundo parágrafo do objetivo.","origin":3}),
            json!({"v":1,"id":3,"at":at(3),"type":"message","author":"user","text":"Quero um painel."}),
            json!({"v":1,"id":4,"at":at(4),"type":"context","author":"assistant","text":"O segundo contexto fica na aba.","origin":3}),
            json!({"v":1,"id":5,"at":at(5),"type":"rule","author":"assistant","text":"O girassol abre a regra.","keys":["flor"],"example":"`x`.","origin":3}),
            json!({"v":1,"id":6,"at":at(6),"type":"note","author":"assistant","text":"O girassol da anotação.","keys":["flor"],"origin":3}),
            json!({"v":1,"id":7,"at":at(7),"type":"decision","author":"assistant","text":"A página vira painel.","why":"Nada repete.","origin":3}),
            json!({"v":1,"id":10,"at":at(10),"type":"wave","author":"binary","n":1,"text":"O lote do chão.","criteria":[20],"done_when":"A suíte passa.","origin":3}),
            json!({"v":1,"id":14,"at":at(14),"type":"send","author":"binary","wave":1,"role":"wave",
                "template":"# molde da onda 1","text":"# pedido da onda 1\n\nFaça o chão.","lines":3,"chars":30,"items":[10],
                "mustard":"0.2.2","steps":21,"tokens":120_000,"caller_tokens":40_000}),
            json!({"v":1,"id":15,"at":at(15),"type":"delivered","author":"wave","wave":1,"text":"O chão entregue.","files":["src/t11.rs","src/t12.rs","src/t13.rs"]}),
            json!({"v":1,"id":16,"at":at(16),"type":"commit","author":"binary","sha":"abc1234","title":"feat: o chão do painel","waves":[1],"files":["src/t11.rs"],"repo":"."}),
            json!({"v":1,"id":17,"at":at(17),"type":"verdict","author":"review","wave":1,"result":"approved","text":"O chão fecha.","final":false}),
            json!({"v":1,"id":30,"at":at(30),"type":"wave","author":"binary","n":2,"text":"O lote do meio.","criteria":[20],"done_when":"A suíte passa.","origin":3}),
            json!({"v":1,"id":32,"at":at(32),"type":"send","author":"binary","wave":2,"role":"wave","text":"# pedido da onda 2","lines":1,"chars":20,"items":[30],"mustard":"0.2.2","steps":8,"tokens":80_000}),
            json!({"v":1,"id":33,"at":at(33),"type":"wave","author":"binary","n":3,"text":"O lote de cima.","criteria":[21],"done_when":"A suíte passa.","origin":3}),
            json!({"v":1,"id":36,"at":at(36),"type":"send","author":"binary","wave":3,"role":"wave","text":"# pedido da onda 3","lines":1,"chars":20,"items":[33],"mustard":"0.2.2","steps":6,"tokens":50_000}),
            json!({"v":1,"id":20,"at":at(20),"type":"criterion","author":"assistant","when":"A página abre.","then":"O painel aparece.","proof":"cargo test painel","origin":3}),
            json!({"v":1,"id":21,"at":at(21),"type":"criterion","author":"assistant","when":"A busca roda.","then":"As abas contam.","proof":"cargo test busca","origin":3}),
            json!({"v":1,"id":22,"at":at(22),"type":"criterion","author":"assistant","when":"O .md baixa.","then":"Nada falta.","proof":"cargo test md","origin":3}),
            json!({"v":1,"id":40,"at":at(40),"type":"criterion_run","author":"binary","criterion":20,"result":"pass","exit":0,"ms":900}),
            json!({"v":1,"id":41,"at":at(41),"type":"criterion_run","author":"binary","criterion":21,"result":"fail","exit":1,"ms":900}),
            json!({"v":1,"id":42,"at":at(42),"type":"criterion_run","author":"binary","criterion":21,"result":"pass","exit":0,"ms":900}),
            json!({"v":1,"id":45,"at":at(45),"type":"note","author":"assistant","text":"Anotação que saiu.","keys":["k"],"origin":3}),
            json!({"v":1,"id":46,"at":at(46),"type":"remove","author":"user","targets":[45],"reason":"Saiu.","origin":3}),
        ];
        lines.extend([
            task(11, Some(1), "O chão da tela", json!([])),
            task(12, Some(1), "A régua dos números", json!([])),
            task(13, Some(1), "O rodapé fixo", json!([])),
            task(31, Some(2), "A lista do meio", json!([])),
            task(34, Some(3), "O gráfico de barras", json!([])),
            task(35, Some(3), "As abas de baixo", json!([])),
            task(39, Some(3), "A legenda das cores", json!([])),
            task(37, None, "A troca de tema", json!([31])),
            task(38, None, "A busca por código", json!([11])),
        ]);
        lines.sort_by_key(|l| l["id"].as_u64().unwrap_or(0));
        lines
    }

    /// O banco do painel: o estado das ondas e os números do gasto que o
    /// binário calcula.
    fn dashboard_database(lines: &[Value]) -> Value {
        json!({"ranges": range_docs(lines), "computed": [{"id": "current", "data": {
            "spec": "painel", "waves": {"1": "approved", "2": "running", "3": "running"}, "prompts": {},
            "rtk": [{"date": "2026-09-22", "commands": 12, "input": 1_000, "saved": 600}],
            "spend": "Gasto total: 250000 tokens de onda + 40000 tokens de quem despachou = 290000 tokens.",
            "tokens": {"waves": 250_000, "caller": 40_000, "total": 290_000, "turns": 5}}}]})
    }

    /// A página da spec é um painel, nos dois idiomas, com cada coisa num
    /// lugar só: o cabeçalho numa linha; o objetivo inteiro; os quatro
    /// quadros com os números; a lista Agora com as ondas que rodam e o
    /// backlog, cada tarefa com o que espera; o gráfico com uma barra por
    /// onda e o detalhe da aberta; a busca acima das abas; as oito abas, uma
    /// aberta, cada item num cartão com o código, o tipo e a data no alto e
    /// o título embaixo. A busca muda a conta de cada aba; o endereço
    /// `#waves-N` abre a onda N; as partes antigas não existem; o título de
    /// uma tarefa aparece uma vez só; e o `.md` baixado tem todo item.
    #[test]
    fn a_pagina_da_spec_e_um_painel_sem_nada_repetido() {
        let lines = dashboard_lines();
        let content = lines.iter().map(Value::to_string).collect::<Vec<_>>().join("\n");
        let codes = parse_log(&content).codes();
        let code = |id: u64| codes.get(&id).cloned().unwrap_or_else(|| panic!("no code for {id}"));
        let task_titles = [
            "O chão da tela", "A régua dos números", "O rodapé fixo", "A lista do meio", "O gráfico de barras",
            "As abas de baixo", "A legenda das cores", "A troca de tema", "A busca por código",
        ];
        struct Words {
            lang: Locale,
            phase: &'static str,
            tiles: [(&'static str, &'static str, &'static [&'static str]); 4],
            now: [Value; 7],
            tabs: [&'static str; 8],
            detail: &'static str,
            commit: &'static str,
            old: [&'static str; 2],
        }
        let pt = Words {
            lang: Locale::PtBr,
            phase: "em execução",
            tiles: [
                ("Ondas", "1 entregue", &["2 rodando agora"]),
                ("Backlog", "2 tarefas", &["1 pronta para sair"]),
                ("Critérios", "2 de 3", &["com a última prova verde"]),
                ("Gasto", "290 mil tokens", &["250 mil de onda + 40 mil de quem despachou", "5 turnos por tarefa", "o rtk poupou 600 (60%)"]),
            ],
            now: [
                json!(["group", "Rodando"]),
                json!(["wave", "ONDA 2", "A lista do meio", "1 arquivo · em andamento"]),
                json!(["wave", "ONDA 3", "3 tarefas", "3 arquivos · em andamento"]),
                json!(["group", "Backlog · tarefas que ainda não viraram onda"]),
                json!(["backlog", "ESPERA", "A troca de tema", "1 arquivo · espera a onda 2"]),
                json!(["backlog", "PRONTA", "A busca por código", "1 arquivo"]),
                json!(["after", "Depois vêm a revisão final e o fechamento."]),
            ],
            tabs: ["Especificação", "Acordado", "Critérios", "Anotações", "Revisão", "Andamento", "Conversa", "Removidos"],
            detail: "aprovada em 2026-09-22 08:15 · 3 arquivos · 120 mil tokens",
            commit: "Commit: feat: o chão do painel",
            old: ["O que falta", "Filtrar por tipo"],
        };
        let en = Words {
            lang: Locale::EnUs,
            phase: "running",
            tiles: [
                ("Waves", "1 delivered", &["2 running now"]),
                ("Backlog", "2 tasks", &["1 ready to go"]),
                ("Criteria", "2 of 3", &["with the last proof green"]),
                ("Spend", "290 k tokens", &["250 k of waves + 40 k of the dispatcher", "5 turns per task", "rtk saved 600 (60%)"]),
            ],
            now: [
                json!(["group", "Running"]),
                json!(["wave", "WAVE 2", "A lista do meio", "1 file · in progress"]),
                json!(["wave", "WAVE 3", "3 tasks", "3 files · in progress"]),
                json!(["group", "Backlog · tasks not yet in a wave"]),
                json!(["backlog", "WAITS", "A troca de tema", "1 file · waits for wave 2"]),
                json!(["backlog", "READY", "A busca por código", "1 file"]),
                json!(["after", "Then come the final review and the closing."]),
            ],
            tabs: ["Specification", "Agreed", "Criteria", "Notes", "Review", "Progress", "Conversation", "Removed"],
            detail: "approved on 2026-09-22 08:15 · 3 files · 120 k tokens",
            commit: "Commit: feat: o chão do painel",
            old: ["What is left", "Filter by type"],
        };
        for words in [pt, en] {
            let lang = words.lang;
            let steps = json!([
                {"do": "wait"}, {"do": "scrape", "as": "page"}, {"do": "download", "as": "md"},
                {"do": "search", "value": "Girassol"}, {"do": "scrape", "as": "search"},
                {"do": "search", "value": ""}, {"do": "hash", "value": "waves-1"}, {"do": "scrape", "as": "wave1"},
                {"do": "bar", "value": 3, "key": "Enter"}, {"do": "scrape", "as": "wave3"},
            ]);
            let got = run("spec", &spec_page_template(lang), Some(dashboard_database(&lines)), steps);
            let page = &got["page"];

            // A ordem do painel e o cabeçalho numa linha.
            assert_eq!(page["blocks"], json!(["goal", "tiles", "now", "chart", "find", "tabs", "panels"]), "{lang}");
            assert_eq!(page["head"], json!(["H1", "SPAN.phase", "SPAN.branch", "DIV.tools"]), "{lang}");
            assert_eq!(
                (&page["title"], &page["phase"], &page["branch"], &page["downloadHidden"]),
                (&json!("painel"), &json!(words.phase), &json!("feature/painel → dev"), &json!(false)),
                "{lang}"
            );
            // O objetivo, inteiro, fora da aba Especificação.
            assert_eq!(page["goal"]["text"], json!("O painel diz cada coisa uma vez.O segundo parágrafo do objetivo."), "{lang}");
            let spec_codes: Vec<&Value> = cards(page, "specification").iter().map(|i| &i["code"]).collect();
            assert_eq!(spec_codes, [&json!(code(4))], "{lang}: the goal leaves the tab");
            // Os quatro quadros.
            let tiles: Vec<Value> = page["tiles"].as_array().expect("tiles").iter().map(|t| json!([t["key"], t["value"], t["lines"]])).collect();
            let expected: Vec<Value> = words.tiles.iter().map(|(k, v, l)| json!([k, v, l])).collect();
            assert_eq!(tiles, expected, "{lang}");
            let meters: Vec<(&Value, &Value)> = page["tiles"].as_array().expect("tiles").iter().map(|t| (&t["meter"], &t["meterWidth"])).collect();
            assert_eq!(
                meters,
                [(&json!(false), &Value::Null), (&json!(false), &Value::Null), (&json!(true), &json!("width:66.7%")), (&json!(false), &Value::Null)],
                "{lang}: only the criteria tile has a bar"
            );
            // O quadro Backlog leva ao grupo Backlog da lista.
            assert_eq!((&page["tiles"][1]["tag"], &page["tiles"][1]["href"]), (&json!("A"), &json!("#backlog")), "{lang}");
            assert_eq!(page["now"]["parts"][3]["id"], json!("backlog"), "{lang}");
            // A lista Agora.
            assert_eq!(now_rows(page), words.now.to_vec(), "{lang}");
            assert_eq!(page["now"]["parts"][2]["tasks"], json!(["O gráfico de barras", "As abas de baixo", "A legenda das cores"]), "{lang}");
            // O gráfico: uma barra por onda, a entregue em verde e as que rodam
            // tracejadas, e a primeira que roda aberta.
            let bars: Vec<Value> = page["chart"]["bars"].as_array().expect("bars").iter().map(|b| json!([b["wave"], b["class"], b["focusable"]])).collect();
            assert_eq!(bars, [json!([1, "b done", true]), json!([2, "b running on", true]), json!([3, "b running", true])], "{lang}");
            let template = spec_page_template(lang);
            assert!(template.contains("rect.b.done") && template.contains("rect.b.running"), "{lang}");
            let running_rule = template.split("rect.b.running").nth(1).and_then(|r| r.split('}').next()).unwrap_or_default();
            assert!(running_rule.contains("stroke-dasharray"), "{lang}: the running bar is dashed: {running_rule}");
            assert_eq!(page["detail"]["wave"], json!(2), "{lang}");
            // A busca, numa linha acima das abas.
            assert_eq!(page["findBeforeTabs"], json!(true), "{lang}");
            // As oito abas, uma aberta; cada cartão com o código, o tipo e a
            // data no alto e o título embaixo.
            let tabs: Vec<&Value> = page["tabs"].as_array().expect("tabs").iter().map(|t| &t["label"]).collect();
            assert_eq!(tabs, words.tabs.iter().map(|t| json!(t)).collect::<Vec<_>>().iter().collect::<Vec<_>>(), "{lang}");
            let selected: Vec<bool> = page["tabs"].as_array().expect("tabs").iter().map(|t| t["selected"] == json!(true)).collect();
            assert_eq!(selected.iter().filter(|s| **s).count(), 1, "{lang}");
            let hidden: Vec<bool> = page["sections"].as_array().expect("sections").iter().map(|s| s["hidden"] == json!(true)).collect();
            assert_eq!(hidden, [false, true, true, true, true, true, true, true], "{lang}");
            for section in page["sections"].as_array().expect("sections") {
                for item in section["items"].as_array().expect("items") {
                    let top: Vec<&str> = item["top"].as_array().expect("top").iter().map(|c| c.as_str().unwrap_or_default()).collect();
                    assert_eq!((top.first(), top.get(1).map(|t| t.starts_with("tag"))), (Some(&"c"), Some(true)), "{lang}: {item}");
                    assert_eq!(top.last(), Some(&"when"), "{lang}: {item}");
                    assert_eq!(item["below"], json!(["top", "t"]), "{lang}: {item}");
                    assert_eq!(item["codeShown"], item["code"], "{lang}");
                    assert!(!item["title"].as_str().unwrap_or_default().is_empty() && !item["date"].is_null(), "{lang}: {item}");
                }
            }
            let criteria: Vec<(&Value, &Value)> = cards(page, "criteria").iter().map(|c| (&c["code"], &c["statusClass"])).collect();
            assert_eq!(
                criteria,
                [(&json!(code(20)), &json!("pill state done")), (&json!(code(21)), &json!("pill state done")), (&json!(code(22)), &json!("pill state wait"))],
                "{lang}"
            );
            // Com a busca, cada aba conta só o que casa.
            let counts: Vec<String> = tab_counts(&got["search"]).into_iter().map(|(_, c)| c).collect();
            assert_eq!(counts, ["0", "1", "0", "1", "0", "0", "0", "0"], "{lang}");
            let before: Vec<String> = tab_counts(page).into_iter().map(|(_, c)| c).collect();
            assert_ne!(before, counts, "{lang}: the search changes the counts");
            // O endereço #waves-1 abre o detalhe da onda entregue, com as
            // tarefas numeradas, o pedido uma vez, as medidas e o commit.
            let wave1 = &got["wave1"];
            let detail = &wave1["detail"];
            assert_eq!((&detail["wave"], &detail["number"], &detail["meta"]), (&json!(1), &json!("1"), &json!(words.detail)), "{lang}");
            assert_eq!(detail["tasks"], json!(["O chão da tela", "A régua dos números", "O rodapé fixo"]), "{lang}");
            let prompts = detail["prompts"].as_array().expect("prompts");
            assert_eq!(prompts.len(), 2, "{lang}: the template and the text, once each: {prompts:?}");
            assert!(prompts[0]["text"].as_str().unwrap_or_default().contains("molde da onda 1"), "{lang}");
            assert!(prompts[1]["text"].as_str().unwrap_or_default().contains("Faça o chão."), "{lang}");
            assert!(detail["measures"].as_array().expect("measures").len() >= 4, "{lang}: {detail}");
            assert_eq!(detail["commit"], json!(words.commit), "{lang}");
            assert_eq!(wave1["chart"]["bars"][0]["class"], json!("b done on"), "{lang}");
            assert_eq!(got["wave3"]["detail"]["wave"], json!(3), "{lang}: Enter on a bar opens it");
            // As partes antigas não existem.
            let classes = format!(" {} ", page["classes"].as_str().unwrap_or_default());
            for gone in ["remaining", "rm-box", "wgrid", "overview", "nav", "side", "menu-btn", "group", "block", "meta", "eyebrow"] {
                assert!(!classes.contains(&format!(" {gone} ")), "{lang}: {gone} is still on the page");
            }
            let ids: Vec<&str> = page["ids"].as_array().expect("ids").iter().map(|i| i.as_str().unwrap_or_default()).collect();
            for gone in ["remaining", "waves", "sections", "type", "notFound"] {
                assert!(!ids.contains(&gone), "{lang}: #{gone} is still on the page");
            }
            assert!(!page["tags"].as_str().unwrap_or_default().split(' ').any(|t| t == "SELECT"), "{lang}: no type filter");
            let text = page["text"].as_str().unwrap_or_default();
            assert!(!text.contains('☰'), "{lang}: no side menu");
            for old in words.old {
                assert!(!text.contains(old), "{lang}: {old:?} is still on the page");
            }
            // Cada onda e cada tarefa num lugar só: nenhum item de onda vira
            // cartão de aba, e o título de cada tarefa aparece uma vez na tela.
            for section in wave1["sections"].as_array().expect("sections") {
                for item in section["items"].as_array().expect("items") {
                    let kind = item["type"].as_str().unwrap_or_default();
                    assert!(!["wave", "task", "send", "delivered", "commit"].contains(&kind), "{lang}: {kind} in a tab: {item}");
                }
            }
            let shown = wave1["shown"].as_str().unwrap_or_default();
            for title in task_titles {
                assert_eq!(shown.matches(title).count(), 1, "{lang}: {title:?} once on screen:\n{shown}");
            }
            for wave_text in ["O lote do chão.", "O lote do meio.", "O lote de cima."] {
                assert!(!shown.contains(wave_text), "{lang}: {wave_text:?} outside the chart and its detail");
            }
            // Cada número dos quadros é dito uma vez só na tela.
            for (_, value, lines) in words.tiles {
                for said in std::iter::once(&value).chain(lines.iter()) {
                    assert_eq!(shown.matches(said).count(), 1, "{lang}: {said:?} said once:\n{shown}");
                }
            }
            // O .md baixado tem todo item da spec, e as abas na ordem.
            let md = got["md"]["data"].as_str().expect("the downloaded .md");
            for id in lines.iter().filter_map(|l| l["id"].as_u64()) {
                let c = code(id);
                assert!(md.contains(&format!("**{c}**")), "{lang}: {c} missing from the .md:\n{md}");
            }
            let at: Vec<usize> = words.tabs.iter().map(|t| md.find(&format!("\n## {t}\n")).unwrap_or_else(|| panic!("{lang}: no {t} in the .md"))).collect();
            assert!(at.windows(2).all(|w| w[0] < w[1]), "{lang}: the tabs in order in the .md: {at:?}");
            assert_md_matches_the_whole_page(md, page);
            // A palavra antiga não aparece na página que a pessoa vê nem no
            // molde publicado. Ela vai em pedaços: o teste do vocabulário cai
            // quando ela aparece inteira em qualquer arquivo do Mustard.
            let seen = text.to_lowercase();
            let template = template.to_lowercase();
            for word in [concat!("ces", "ta"), concat!("bas", "ket")] {
                assert!(!seen.contains(word), "{lang}: {word} on the page");
                assert!(!template.contains(word), "{lang}: {word} in the template");
            }
        }
    }

    /// A coluna da página da spec tem no máximo 1040 pixels e fica no meio
    /// da tela, como a da página do projeto; no celular, o recuo de 12
    /// pixels continua. O gráfico das ondas tem altura fixa de 200 pixels e
    /// a largura segue o desenho: não cresce com a largura da tela nem com
    /// o número de barras, e a moldura rola de lado quando as barras não
    /// cabem. Nenhuma regra do celular devolve ao gráfico uma largura mínima.
    #[test]
    fn a_pagina_da_spec_tem_coluna_de_1040_e_grafico_de_altura_fixa() {
        // As declarações de cada regra do molde com exatamente esse seletor,
        // na ordem, contando também as de dentro de um @media, numa linha
        // própria ou na mesma linha dele. Um seletor mais longo que termina
        // igual, como `.x .chart svg`, não conta.
        let rules = |selector: &str| -> Vec<Vec<&str>> {
            let open = format!("{selector}{{");
            SPEC_PAGE
                .match_indices(&open)
                .filter(|(at, _)| {
                    let before = &SPEC_PAGE[..*at];
                    let start = before.rfind(['\n', '{', '}']).map_or(0, |i| i + 1);
                    before[start..].trim().is_empty()
                })
                .filter_map(|(at, _)| SPEC_PAGE[at + open.len()..].split('}').next())
                .map(|body| body.split(';').filter(|d| !d.is_empty()).collect())
                .collect()
        };

        let page = rules(".page");
        assert_eq!(page.len(), 2, "the column rule and the phone indent: {page:?}");
        for decl in ["max-width:1040px", "margin-inline:auto"] {
            assert!(page[0].contains(&decl), "the column lacks {decl}: {:?}", page[0]);
        }
        assert_eq!(page[1], ["padding-inline:12px"], "the phone keeps its 12 pixel indent");

        assert_eq!(rules(".chart"), [["overflow-x:auto"]], "the chart frame scrolls sideways");
        let svg = rules(".chart svg");
        assert_eq!(svg.len(), 1, "a single rule sizes the chart, none on the phone: {svg:?}");
        for decl in ["height:200px", "width:auto"] {
            assert!(svg[0].contains(&decl), "the chart lacks {decl}: {:?}", svg[0]);
        }
        for decl in &svg[0] {
            assert!(
                !decl.starts_with("min-width") && *decl != "height:auto" && *decl != "width:100%",
                "the chart would grow with the screen or the bars: {decl}",
            );
        }
        // Cada onda continua ocupando 16 unidades do desenho: 12 de barra e 4
        // de espaço.
        assert!(SPEC_PAGE.contains("bw = 12, gap = 4"), "the drawing keeps 16 units per wave");
    }

    /// As cores do painel moram em variáveis, com o tema escuro pelo sistema
    /// e pela escolha da página, e o painel cabe na tela do celular.
    #[test]
    fn o_painel_segue_o_tema_e_cabe_no_celular() {
        let html = spec_page_template(Locale::PtBr);
        for piece in [
            ":root{",
            "@media (prefers-color-scheme: dark){:root:not([data-theme=\"light\"]){",
            ":root[data-theme=\"dark\"]{",
            "color-scheme:dark",
            "@media (max-width:560px)",
        ] {
            assert!(html.contains(piece), "{piece:?} is not in the template");
        }
        let body = html.split("\nbody{").nth(1).and_then(|b| b.split('}').next()).unwrap_or_default();
        assert!(body.contains("background"), "the body has its own background: {body}");
    }

    /// Todo texto que os templates citam existe nos dois idiomas, e o
    /// catálogo entra no lugar dele: o template publicado não tem o lugar
    /// vazio.
    #[test]
    fn every_text_the_templates_cite_exists_in_both_languages() {
        for (name, template) in [("spec", SPEC_PAGE), ("project", PROJECT_PAGE)] {
            assert!(template.contains(CATALOG_SLOT), "{name} has the catalog slot");
            for key in cited_keys(template).into_iter().filter(|k| !k.ends_with('.')) {
                for lang in [Locale::PtBr, Locale::EnUs] {
                    assert_ne!(translate(&key, lang), "<missing-key>", "{name} cites {key}, missing in {lang}");
                }
            }
        }
        for lang in [Locale::PtBr, Locale::EnUs] {
            for filled in [spec_page_template(lang), project_page_template(lang)] {
                assert!(!filled.contains(CATALOG_SLOT), "the catalog is in place");
                assert!(filled.contains(&format!("\"lang\":\"{}\"", lang.as_str())));
            }
        }
    }
}
