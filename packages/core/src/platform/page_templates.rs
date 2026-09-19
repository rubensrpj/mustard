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
//! - A coleção [`ITEMS`] tem um documento por item do `spec.ndjson`, com o
//!   número do item como nome do documento e a linha do item, como está no
//!   arquivo, como corpo (`id`, `code`, `at`, `type`, `author` e os campos do
//!   tipo). O template lê os itens em ordem de número, em páginas.
//! - O documento [`COMPUTED`] guarda o que o binário calcula e o
//!   `spec.ndjson` não tem, trocado a cada cópia: `spec` (o nome da spec),
//!   `waves` (o estado de cada onda pelo número dela: `todo`, `running`,
//!   `delivered`, `approved` ou `rejected`), `prompts` (o pedido de cada onda
//!   que ainda não saiu, pelo número dela) e `rtk` (a economia do rtk, um dia
//!   por linha, com `date`, `commands`, `input` e `saved`).
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
use crate::platform::i18n::{translate, Locale};

/// O template da página da spec, como mora no repositório.
const SPEC_PAGE: &str = include_str!("../../templates/pages/spec.html");

/// O template da página do projeto, como mora no repositório.
const PROJECT_PAGE: &str = include_str!("../../templates/pages/project.html");

/// O lugar do catálogo em cada template, vazio até o binário preenchê-lo.
const CATALOG_SLOT: &str = r#"<script type="application/json" id="mustard-catalog">{}</script>"#;

/// A coleção dos itens da spec no banco da página dela.
pub const ITEMS: &str = "items";

/// O documento das coisas calculadas no banco da página da spec.
pub const COMPUTED: &str = "computed/current";

/// A coleção das specs no banco da página do projeto.
pub const SPECS: &str = "specs";

/// O que a página da spec declara ao ser publicada: o banco de dados, que só
/// quem edita a página grava, e o salvar arquivo do botão de baixar o `.md`.
pub const SPEC_CAPABILITIES: &str =
    r#"{"db":{"rules":[{"path":"","read":"view","write":"admin"}]},"downloads":{}}"#;

/// O que a página do projeto declara ao ser publicada: o banco de dados, que
/// só quem edita a página grava.
pub const PROJECT_CAPABILITIES: &str = r#"{"db":{"rules":[{"path":"","read":"view","write":"admin"}]}}"#;

/// O template da página da spec, com o catálogo no idioma `lang`.
#[must_use]
pub fn spec_page_template(lang: Locale) -> String {
    let finding_labels = [Locale::PtBr, Locale::EnUs].map(|l| translate("plan.finding.label", l));
    let catalog = json!({
        "lang": lang.as_str(),
        "db": { "items": ITEMS, "computed": COMPUTED },
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
    template.replacen(CATALOG_SLOT, &filled, 1)
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
    const WAVE_3_PROMPT: &str = "# demo — onda 3\n\n## Especificação\n\n- `specification`: MSTD-CTX-0001, MSTD-CONC-0001\n\n## Combinado\n\n- `agreed`: MSTD-RULE-0001, MSTD-DEC-0001\n\n## Critérios\n\n- `criteria`: MSTD-CRIT-0001\n";

    /// A spec de exemplo mais o que a página nova precisa mostrar: uma regra
    /// revista com duas linhas de lista, uma onda enviada no formato de hoje
    /// e uma onda ainda por enviar.
    fn spec_lines() -> Vec<Value> {
        let extra = [
            json!({"v":1,"id":40,"at":"2026-09-12T12:00:00-03:00","type":"wave","author":"assistant","n":3,"text":"Os templates leem o banco.","criteria":[19],"done_when":"A suíte passa.","origin":2}),
            json!({"v":1,"id":41,"at":"2026-09-12T12:01:00-03:00","type":"task","author":"assistant","wave":3,"text":"Template da página da spec.","files":[{"path":"packages/core/templates/pages/spec.html","new":true}],"points":8,"origin":2}),
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
            (4, "# demo — onda 4\n\n## Combinado\n\n- `agreed`: MSTD-RULE-0001\n".to_string()),
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

    /// O banco da página da spec, como a cópia o deixa: um documento por
    /// item, com o número como nome, e o documento das coisas calculadas.
    fn spec_database(lines: &[Value]) -> Value {
        let items: Vec<Value> = lines.iter().map(|line| json!({"id": line["id"].to_string(), "data": line})).collect();
        let waves: serde_json::Map<String, Value> =
            wave_states().into_iter().map(|(n, s)| (n.to_string(), json!(state_name(s)))).collect();
        let prompts: serde_json::Map<String, Value> =
            wave_prompts().into_iter().map(|(n, p)| (n.to_string(), json!(p))).collect();
        let rtk: Vec<Value> = rtk_days()
            .into_iter()
            .map(|d| json!({"date": d.date, "commands": d.commands, "input": d.input, "saved": d.saved}))
            .collect();
        json!({
            "items": items,
            "computed": [{"id": "current", "data": {"spec": "demo", "waves": waves, "prompts": prompts, "rtk": rtk}}],
        })
    }

    /// Roda `html` no Node com o banco `db` e os passos `steps`, e devolve o
    /// que a página mostrou em cada passo.
    fn run(page: &str, html: &str, db: Option<Value>, steps: Value) -> Value {
        let mut child = Command::new("node")
            .arg(HARNESS)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the page templates run in Node.js during the test: install Node and put `node` on the PATH");
        let input = json!({"page": page, "html": html, "db": db, "steps": steps}).to_string();
        child.stdin.take().expect("stdin").write_all(input.as_bytes()).expect("the harness reads its input");
        let out = child.wait_with_output().expect("the harness ends");
        assert!(out.status.success(), "the harness failed: {}", String::from_utf8_lossy(&out.stderr));
        let got: Value = serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stdout)));
        assert_eq!(got["errors"], json!([]), "the page script failed");
        got
    }

    /// A mesma forma que o arquivo fixo guarda, lida da página do template. A
    /// seção dos removidos fica de fora: a página que o motor antigo montava
    /// não a tinha.
    fn page_seen(seen: &Value) -> Value {
        let sections: Vec<Value> = seen["sections"]
            .as_array()
            .expect("sections")
            .iter()
            .filter(|s| s["id"] != json!("removed"))
            .map(|s| {
                let groups: Vec<Value> = s["groups"]
                    .as_array()
                    .expect("groups")
                    .iter()
                    .map(|g| {
                        let items: Vec<Value> = g["items"]
                            .as_array()
                            .expect("items")
                            .iter()
                            .map(|i| {
                                let fields: Vec<Value> =
                                    i["fields"].as_array().expect("fields").iter().map(|f| json!([f[0], f[1]])).collect();
                                json!({"code": i["code"], "anchored": i["anchored"], "title": i["title"], "who": i["who"],
                                    "mark": i["mark"], "status": i["status"], "date": i["date"], "fields": fields})
                            })
                            .collect();
                        json!({"id": g["id"], "title": g["title"], "summary": g["summary"], "items": items})
                    })
                    .collect();
                json!({"id": s["id"], "heading": s["heading"], "groups": groups})
            })
            .collect();
        json!(sections)
    }

    fn visible_codes(seen: &Value) -> Vec<String> {
        let mut out = Vec::new();
        for section in seen["sections"].as_array().expect("sections") {
            for group in section["groups"].as_array().expect("groups") {
                for item in group["items"].as_array().expect("items") {
                    if item["hidden"] == json!(false) {
                        out.push(item["code"].as_str().unwrap_or_default().to_string());
                    }
                }
            }
        }
        out
    }

    fn prompt_of<'a>(seen: &'a Value, group: &str) -> &'a Value {
        seen["sections"]
            .as_array()
            .expect("sections")
            .iter()
            .flat_map(|s| s["groups"].as_array().expect("groups").iter())
            .find(|g| g["id"] == json!(group))
            .and_then(|g| g["prompts"].as_array().and_then(|p| p.first()))
            .unwrap_or_else(|| panic!("no prompt in {group}"))
    }

    /// Os dois templates leem o banco de dados da página. O da spec mostra as
    /// mesmas seções, grupos e itens da página que o binário montava, com o
    /// estado de cada onda e o pedido completo de cada uma (o texto de cada
    /// item no lugar do código); a busca e o filtro por tipo escondem o que
    /// não serve; o botão baixa o `.md` com o mesmo conteúdo. O do projeto
    /// mostra as specs agrupadas por fase, com o link da página de cada uma.
    #[test]
    fn the_page_templates_read_the_database() {
        let lines = spec_lines();
        let steps = json!([
            {"do": "wait"}, {"do": "scrape", "as": "page"},
            {"do": "download", "as": "md"},
            {"do": "search", "value": "powershell"}, {"do": "scrape", "as": "search"},
            {"do": "search", "value": ""}, {"do": "filter", "value": "decision"}, {"do": "scrape", "as": "filter"},
            {"do": "search", "value": "windows"}, {"do": "scrape", "as": "none"},
            {"do": "filter", "value": "deferred"}, {"do": "scrape", "as": "both"},
        ]);
        let got = run("spec", &spec_page_template(Locale::PtBr), Some(spec_database(&lines)), steps);
        let page = &got["page"];

        assert_eq!(page["state"], json!("ready"), "the page read the database");
        assert_eq!(page["statusHidden"], json!(true));
        assert_eq!(page["title"], json!("demo"));
        assert_eq!(page["meta"], json!(["spec demo", "fase aprovada", "branch feature/demo", "sai de dev"]));
        let fixed: Value = serde_json::from_str(SPEC_PAGE_FIXTURE).expect("the spec page fixture is valid JSON");
        assert_eq!(page_seen(page), fixed, "the same sections, groups and items as the fixed page");
        // A versão mais nova de cada item, sem o item retirado, com a marca do
        // que entrou depois da aprovação; a versão antiga fica na conversa. O
        // item retirado só aparece na seção dos removidos.
        let codes: Vec<String> = page["sections"]
            .as_array()
            .expect("sections")
            .iter()
            .filter(|s| s["id"] != json!("removed"))
            .flat_map(|s| s["groups"].as_array().expect("groups").iter())
            .flat_map(|g| g["items"].as_array().expect("items").iter())
            .map(|i| i["code"].as_str().unwrap_or_default().to_string())
            .collect();
        assert!(!codes.contains(&"MSTD-NOTE-0001".to_string()), "the removed note is gone");
        let rules = page["sections"][2]["groups"].as_array().expect("agreed groups").iter().find(|g| g["id"] == json!("agreed-rule")).expect("rules");
        assert_eq!(rules["items"].as_array().map(Vec::len), Some(1));
        assert_eq!(rules["items"][0]["title"], json!("A trava de comandos confere o programa, as opções e o caminho."));
        assert_eq!(rules["items"][0]["mark"], json!("depois da aprovação"));
        let talk = page["sections"].as_array().expect("sections").iter().find(|s| s["id"] == json!("conversation")).expect("talk");
        let old = talk["groups"]
            .as_array()
            .expect("days")
            .iter()
            .flat_map(|g| g["items"].as_array().expect("items").iter())
            .find(|i| i["code"] == json!("MSTD-RULE-0001"))
            .expect("the old version of the rule");
        assert_eq!((&old["status"], &old["anchored"]), (&json!("versão antiga"), &json!(false)));
        let headings: Vec<&str> =
            page["sections"].as_array().expect("sections").iter().map(|s| s["id"].as_str().unwrap_or_default()).collect();
        assert_eq!(
            headings,
            ["progress", "specification", "agreed", "criteria", "waves", "review", "findings", "notes", "conversation", "removed"]
        );
        let overview = &page["sections"][0]["overview"];
        assert_eq!(overview["legend"], json!("2 a fazer · 1 aprovada · 1 em andamento"), "the wave states come from the database");

        // O pedido completo: cada código vira o texto da versão mais nova do
        // item, com as linhas de lista dele.
        let sent = prompt_of(page, "waves-3");
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
        assert!(prompt_of(page, "waves-4")["text"].as_str().unwrap_or_default().contains("MSTD-RULE-0001 — A trava de comandos"));

        // A escolha do orquestrador gravada no envio: o item e a lição que
        // saíram do pedido, e o item que entrou, cada um com o motivo.
        let send_item = page["sections"][4]["groups"][2]["items"][2].clone();
        assert_eq!(send_item["code"], json!("MSTD-SEND-0002"));
        let analysis_field = send_item["fields"]
            .as_array()
            .expect("fields")
            .iter()
            .find(|f| f[0] == json!("Análise antes do envio"))
            .unwrap_or_else(|| panic!("no analysis field: {send_item}"));
        assert_eq!(
            analysis_field[1],
            json!(
                "Tirou do pedido: MSTD-RULE-0001 (A regra fala da trava, não da tabela desta onda.); \
                 lição 12 (A lição é de outra onda.) · Pôs no pedido: MSTD-CTX-0001 (O contexto explica \
                 por que a tabela nasce vazia.)"
            ),
            "{send_item}"
        );

        // O veredito final do agente de teste dedicado ganha grupo próprio no
        // bloco de revisão, mesmo apontando a mesma onda 2 do outro veredito:
        // o grupo da onda 2 continua só com o veredito dela.
        let review = page["sections"].as_array().expect("sections").iter().find(|s| s["id"] == json!("review")).expect("review");
        let review_groups: Vec<&str> =
            review["groups"].as_array().expect("groups").iter().map(|g| g["id"].as_str().unwrap_or_default()).collect();
        assert_eq!(review_groups, ["review-2", "review-final"], "{review}");
        let wave_2_group = &review["groups"][0];
        assert_eq!(wave_2_group["items"].as_array().map(Vec::len), Some(1), "the final verdict stays out: {wave_2_group}");
        let final_group = &review["groups"][1];
        assert_eq!(final_group["title"], json!("Veredito final"));
        assert_eq!(final_group["summary"], json!("1 aprovada"));
        let final_item = &final_group["items"][0];
        let final_fields: Vec<Value> =
            final_item["fields"].as_array().expect("fields").iter().map(|f| json!([f[0], f[1]])).collect();
        assert_eq!(
            (
                final_item["code"].clone(),
                final_item["title"].clone(),
                final_item["status"].clone(),
                final_fields,
            ),
            (
                json!("MSTD-VERD-0002"),
                json!("As ondas se encaixam sem prova perdida."),
                json!("aprovada"),
                vec![json!(["Onda", "2"]), json!(["Resultado", "aprovada"]), json!(["Revisão final", "sim"])],
            ),
            "{final_item}"
        );

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

        // A busca e o filtro por tipo.
        assert_eq!(visible_codes(&got["search"]), ["MSTD-RULE-0001"]);
        assert_eq!(got["search"]["hits"], json!("1 item"));
        let filter: Vec<Value> = page["filter"].as_array().expect("the type filter").clone();
        assert_eq!(filter[0], json!(["", "Todos os tipos"]));
        assert!(filter.contains(&json!(["decision", "decisão"])), "{filter:?}");
        // A decisão vigente e, na conversa, a versão antiga dela.
        assert_eq!(visible_codes(&got["filter"]), ["MSTD-DEC-0001", "MSTD-DEC-0001"]);
        assert_eq!(visible_codes(&got["none"]), Vec::<String>::new(), "no decision talks about Windows");
        assert_eq!(got["none"]["notFound"], json!(true));
        assert_eq!(visible_codes(&got["both"]), ["MSTD-DEFER-0001"], "search and filter hold together");
        assert_eq!(got["both"]["notFound"], json!(false));
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

    /// No fim da página da spec, a seção Removidos mostra cada item que saiu:
    /// o removido com o código, o texto, quem o removeu, quando e por quê; o
    /// expurgado por segredo só com a marca no lugar do texto. Vale para a
    /// tela e para o `.md` baixado. O item removido com as duas versões
    /// aparece uma vez, pela mais nova, e o item expurgado pelo formato de hoje
    /// continua à mostra na seção dele, com o resto do texto.
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
        let sections = got["page"]["sections"].as_array().expect("sections");

        // A seção é a última da página.
        let last = sections.last().expect("a last section");
        assert_eq!((&last["id"], &last["heading"]), (&json!("removed"), &json!("Removidos")));
        let entries: Vec<Value> = last["groups"][0]["items"]
            .as_array()
            .expect("the removed items")
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
                json!({"code": code(34), "who": "anotação", "mark": "removido",
                    "title": "Anotação colada por engano.", "text": "Anotação colada por engano.",
                    "fields": [["Removido por", "assistente"], ["Removido em", "2026-09-12 11:10"],
                        ["Motivo", "Colada por engano."], ["Registro", code(36)]]}),
                json!({"code": code(37), "who": "mensagem", "mark": "expurgado", "title": "…", "text": "…",
                    "fields": [["Expurgado por", "assistente"], ["Expurgado em", "2026-09-12 11:12"],
                        ["Motivo", "segredo"], ["Registro", code(38)]]}),
                json!({"code": code(42), "who": "regra", "mark": "removido",
                    "title": "A trava de comandos confere o programa, as opções e o caminho.",
                    "text": "A trava de comandos confere o programa, as opções e o caminho.vale para o Bash;vale para o PowerShell.",
                    "fields": [["Removido por", "usuário"], ["Removido em", "2026-09-12 14:02"],
                        ["Motivo", "A trava mudou de lugar."], ["Registro", code(52)]]}),
                json!({"code": code(50), "who": "anotação", "mark": "expurgado", "title": "…", "text": "…",
                    "fields": [["Expurgado por", "usuário"], ["Expurgado em", "2026-09-12 14:01"],
                        ["Motivo", "segredo"], ["Registro", code(51)]]}),
            ],
            "each item that left, the purged ones only with the mark"
        );
        assert_eq!(code(9), code(42), "the two versions of the rule are one item");
        // O expurgo de hoje não tira o item da leitura: ele segue nas
        // anotações com o resto do texto, e a seção dos removidos não o repete.
        let notes = sections.iter().find(|s| s["id"] == json!("notes")).expect("notes");
        let kept = notes["groups"][0]["items"].as_array().expect("notes").iter().find(|i| i["code"] == json!(code(50)));
        assert_eq!(kept.map(|i| i["text"].clone()), Some(json!("A chave … fica no cofre do time.")));
        assert!(!last.to_string().contains("cofre"), "the purged text stays out of the removed section: {last}");

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

    /// Uma cópia nova chega com a página aberta: o item novo aparece, o item
    /// que saiu do banco some e o estado novo da onda vale, sem recarregar.
    #[test]
    fn a_new_copy_updates_the_open_page() {
        let lines = spec_lines();
        let note = json!({"v":1,"id":45,"at":"2026-09-12T13:00:00-03:00","type":"note","author":"assistant","code":"MSTD-NOTE-0009","text":"Nota que chegou depois.","keys":["nota"],"origin":2});
        let steps = json!([
            {"do": "wait"}, {"do": "scrape", "as": "before"},
            {"do": "copy", "set": {"items": [{"id": "45", "data": note}],
                "computed": [{"id": "current", "data": {"spec": "demo", "waves": {"2": "approved", "3": "delivered"}}}]},
                "delete": {"items": ["35"]}},
            {"do": "scrape", "as": "after"},
        ]);
        let got = run("spec", &spec_page_template(Locale::PtBr), Some(spec_database(&lines)), steps);
        let notes = |seen: &Value| -> Vec<String> {
            let section = seen["sections"].as_array().expect("sections").iter().find(|s| s["id"] == json!("notes")).expect("notes");
            section["groups"][0]["items"].as_array().expect("items").iter().map(|i| i["title"].as_str().unwrap_or_default().to_string()).collect()
        };
        assert!(notes(&got["before"]).iter().any(|t| t.starts_with("O pull request 276")));
        assert_eq!(
            notes(&got["after"]),
            [
                "Incluir o Windows no teste de duas gravações ao mesmo tempo.",
                "Medir o antivírus do Windows na verificação automática.",
                "Nota que chegou depois.",
            ],
            "the new note came in and the deleted one left"
        );
        let legend = &got["after"]["sections"][0]["overview"]["legend"];
        assert_eq!(legend, &json!("2 a fazer · 1 aprovada · 1 entregue"));
    }

    /// Uma spec longa é lida inteira, em páginas de 500 itens, em ordem de
    /// número: 1.200 itens pedem três leituras.
    #[test]
    fn a_long_spec_is_read_in_pages() {
        let mut lines = spec_lines();
        let first = 100;
        lines.extend((0..1_200).map(|i| {
            json!({"v":1,"id":first + i,"at":"2026-09-13T10:00:00-03:00","type":"note","author":"assistant","text":format!("Nota {i}."),"keys":["k"],"origin":2})
        }));
        let steps = json!([{"do": "wait"}, {"do": "scrape", "as": "page"}, {"do": "reads", "as": "reads"}]);
        let got = run("spec", &spec_page_template(Locale::PtBr), Some(spec_database(&lines)), steps);
        let notes = got["page"]["sections"].as_array().expect("sections").iter().find(|s| s["id"] == json!("notes")).expect("notes");
        assert_eq!(notes["groups"][0]["items"].as_array().map(Vec::len), Some(1_203), "every note of the long spec");
        let pages: Vec<(Value, Value)> = got["reads"]
            .as_array()
            .expect("reads")
            .iter()
            .filter(|r| r["path"] == json!(ITEMS))
            .map(|r| (r["filters"][0][2].clone(), r["size"].clone()))
            .collect();
        let last_of_first_page = lines.iter().map(|l| l["id"].as_u64().unwrap_or(0)).collect::<Vec<_>>()[499];
        assert_eq!(pages.len(), 3, "{pages:?}");
        assert_eq!(pages[0], (json!(-1), json!(500)));
        assert_eq!(pages[1], (json!(last_of_first_page), json!(500)));
        assert_eq!(pages[2].1, json!(lines.len() - 1_000));
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
