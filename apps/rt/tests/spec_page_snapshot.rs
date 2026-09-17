//! A página e o `.md` de uma spec saem do `spec.ndjson` pelo binário, iguais
//! byte a byte a cada vez, em qualquer pasta, e iguais ao retrato guardado em
//! `tests/fixtures/spec_page/`. O arquivo de eventos do retrato tem os 33
//! tipos, uma decisão revista, um item removido e uma mensagem expurgada.
//!
//! A página sai no layout aprovado: o menu lateral com as seções e os
//! grupos, a barra com a busca, os grupos recolhidos com a contagem, cada
//! item numa linha recolhida, e a visão das ondas com o estado que a rodada
//! lê. A conferência no navegador sem janela abre a página e busca um código.
//!
//! Depois de uma mudança de propósito na página, refaça o retrato rodando este
//! teste com `MUSTARD_BLESS=1` e confira a diferença no git.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/spec_page").join(name)
}

/// Um projeto novo com a spec `demo` do retrato e os eventos a mais de
/// `extra`; devolve o `.md` e o `.html` que o `page --spec` gerou, duas vezes
/// seguidas.
fn generate_with(root: &Path, extra: &str) -> [(String, String); 2] {
    fs::write(root.join("mustard.json"), r#"{"language":{"text":"pt-BR"}}"#).expect("config");
    let spec = root.join(".claude").join("spec").join("demo");
    fs::create_dir_all(&spec).expect("spec dir");
    let events = fs::read_to_string(fixture("spec.ndjson")).expect("events");
    fs::write(spec.join("spec.ndjson"), format!("{events}{extra}")).expect("events");
    [0, 1].map(|_| {
        let out = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
            .args(["run", "page", "--spec", "demo", "--root"])
            .arg(root)
            .current_dir(root)
            .output()
            .expect("run page");
        let report: Value = serde_json::from_slice(&out.stdout).expect("a JSON report");
        assert!(out.status.success(), "{report}");
        assert_eq!(report["md"], ".claude/spec/demo/spec.md", "{report}");
        assert_eq!(report["html"], ".claude/spec/demo/spec.html", "{report}");
        let read = |name: &str| fs::read_to_string(spec.join(name)).expect(name);
        (read("spec.md"), read("spec.html"))
    })
}

fn generate(root: &Path) -> [(String, String); 2] {
    generate_with(root, "")
}

fn matches_the_snapshot(name: &str, got: &str) {
    let path = fixture(name);
    if std::env::var_os("MUSTARD_BLESS").is_some() {
        fs::write(&path, got).expect("bless");
        return;
    }
    let want = fs::read_to_string(&path).unwrap_or_default();
    assert!(want == got, "{name} changed; rerun with MUSTARD_BLESS=1 if it was on purpose\n{got}");
}

#[test]
fn the_spec_md_and_page_are_the_same_bytes_every_time() {
    let events = fs::read_to_string(fixture("spec.ndjson")).expect("fixture");
    let types: BTreeSet<String> = events
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| v["type"].as_str().map(str::to_string))
        .collect();
    assert_eq!(types.len(), 33, "the fixture holds every event type: {types:?}");

    let (one, two) = (tempfile::tempdir().expect("tempdir"), tempfile::tempdir().expect("tempdir"));
    let [first, again] = generate(one.path());
    let [elsewhere, _] = generate(two.path());
    assert!(first == again, "two runs over the same events differ");
    assert!(first == elsewhere, "the folder changed the output");

    let (md, html) = first;
    for page in [&md, &html] {
        let folder = one.path().to_string_lossy();
        assert!(!page.contains(folder.as_ref()), "a machine path leaked into the page");
    }
    matches_the_snapshot("spec.md", &md);
    matches_the_snapshot("spec.html", &html);
}

/// A página da spec de teste sai no layout aprovado: o menu lateral com as
/// seções e os grupos, a barra com a busca, os grupos recolhidos com a
/// contagem (só a medição aberta) e cada item numa linha recolhida com o
/// código, o título, a situação e a data. A visão das ondas mostra o estado
/// pela leitura da rodada: a onda aprovada aparece aprovada, e a mesma onda
/// reprovada depois aparece reprovada. A do projeto sai na mesma moldura.
#[test]
fn the_page_has_the_approved_layout_and_the_round_wave_states() {
    let dir = tempfile::tempdir().expect("tempdir");
    let [(_, html), _] = generate(dir.path());
    for piece in [
        "<aside class=\"side\" id=\"side\" aria-label=\"Seções\">",
        "<li data-sec=\"agreed\"><button type=\"button\" data-go=\"agreed\" class=\"top\"><span>Combinado</span><i>9</i></button><ol>",
        "<li><button type=\"button\" data-go=\"agreed-rule\" data-parent=\"agreed\"><span>Regras</span><i>1</i></button></li>",
        "<div class=\"bar\">",
        "<input id=\"q\" type=\"search\" placeholder=\"Buscar texto ou código\"",
        "<details class=\"group\" id=\"agreed-rule\" data-crumb=\"Combinado / Regras\"><summary><span class=\"gt\">Regras</span><span class=\"gs\"></span><span class=\"count\">1 item</span></summary>",
        "<details class=\"item\" id=\"MSTD-VERD-0001\"><summary><code class=\"c\">MSTD-VERD-0001</code><span class=\"t\">Sem achados.</span><span class=\"tail\"><span class=\"tag ok\">aprovada</span><span class=\"when\">12/09 10:10</span></span></summary>",
        "<li><button type=\"button\" class=\"w todo\" data-go=\"waves-1\" title=\"A trava lê o comando como o terminal.\"><b>1</b><span>a fazer</span></button></li>",
        "<li><button type=\"button\" class=\"w ok\" data-go=\"waves-2\" title=\"A aprovação e as pendências leem o estado.\"><b>2</b><span>aprovada</span></button></li>",
        "<span class=\"gs\"><span class=\"tag ok\">aprovada</span> A aprovação e as pendências leem o estado.</span>",
        "<script>",
    ] {
        assert!(html.contains(piece), "{piece} is missing from the page");
    }
    let groups: Vec<&str> = html.match_indices("<details class=\"group\"").map(|(at, _)| &html[at..]).collect();
    assert!(groups.len() > 10, "{} groups", groups.len());
    let open: Vec<&str> = groups
        .iter()
        .filter(|tail| tail.split_once('>').is_some_and(|(head, _)| head.ends_with(" open")))
        .map(|tail| tail.split('"').nth(3).unwrap_or_default())
        .collect();
    assert_eq!(open, ["progress-metrics"], "only the measurement opens");
    for row in html.match_indices("<details class=\"item").map(|(at, _)| &html[at..]) {
        let summary = row.split_once("</summary>").map_or("", |(head, _)| head);
        for part in ["<code class=\"c\">", "<span class=\"t\">", "<span class=\"tail\">"] {
            assert!(summary.contains(part), "a row without {part}: {summary}");
        }
    }

    // A mesma onda, reprovada por uma revisão mais nova, aparece reprovada:
    // o estado sai da leitura da rodada, não de um veredito aprovado antigo.
    let later = tempfile::tempdir().expect("tempdir");
    let rejected = concat!(
        r#"{"v":1,"id":40,"at":"2026-09-12T12:00:00-03:00","type":"verdict","author":"review","wave":2,"result":"rejected","text":"Voltou.","criteria":[{"criterion":19,"tests_rule":true}]}"#,
        "\n"
    );
    let [(_, html), (_, again)] = generate_with(later.path(), rejected);
    assert!(html == again, "the page is the same bytes twice");
    assert!(
        html.contains("<li><button type=\"button\" class=\"w no\" data-go=\"waves-2\" title=\"A aprovação e as pendências leem o estado.\"><b>2</b><span>reprovada</span></button></li>"),
        "the rejected wave is not shown rejected"
    );
    assert!(html.contains("<span class=\"gs\"><span class=\"tag no\">reprovada</span> A aprovação e as pendências leem o estado.</span>"));

    let project = fs::read_to_string(later.path().join(".claude/spec/project.html")).expect("the project page");
    for piece in [
        "<p class=\"brand\"><b>Mustard</b> · projeto</p>",
        "<li data-sec=\"specs\"><button type=\"button\" data-go=\"specs\" class=\"top\"><span>Specs</span><i>1</i></button>",
        "<input id=\"q\" type=\"search\"",
        "<details class=\"group\" id=\"specs-approved\" data-crumb=\"Specs / Aprovada\"><summary><span class=\"gt\">Aprovada</span><span class=\"gs\"></span><span class=\"count\">1 item</span></summary>",
        "<details class=\"item\"><summary><code class=\"c\">demo</code>",
        "<span class=\"tag ok\">aprovada</span><span class=\"when\">12/09 12:00</span>",
    ] {
        assert!(project.contains(piece), "{piece} is missing from the project page:\n{project}");
    }
}

/// O script que a conferência acrescenta a uma cópia da página: busca
/// `term`, espera a busca terminar e escreve num `<pre id="probe">` o que
/// ficou à vista.
fn probe(term: &str) -> String {
    format!(
        r"<script>
setTimeout(function(){{
  var q=document.getElementById('q');
  q.value={term:?};
  q.dispatchEvent(new Event('input'));
  setTimeout(function(){{
    var shown=[].slice.call(document.querySelectorAll('details.item')).filter(function(d){{return !d.hidden;}});
    var groups=[].slice.call(document.querySelectorAll('details.group')).filter(function(g){{return !g.hidden;}});
    var out={{
      items: shown.map(function(d){{return (d.id||'?')+(d.open?' open':' closed');}}),
      marked: shown.map(function(d){{return [].slice.call(d.querySelectorAll('mark.hl')).map(function(m){{return m.textContent;}}).join('|');}}),
      groups: groups.map(function(g){{return g.id+(g.open?' open':' closed');}}),
      hits: document.getElementById('hits').textContent
    }};
    var pre=document.createElement('pre');pre.id='probe';pre.textContent=JSON.stringify(out);
    document.body.appendChild(pre);
  }},600);
}},100);
</script>"
    )
}

/// Abre `page` no navegador sem janela e devolve o que a conferência
/// escreveu.
fn browse(page: &Path, profile: &Path) -> Value {
    let out = Command::new("google-chrome")
        .args(["--headless=new", "--no-sandbox", "--disable-gpu", "--window-size=1280,860", "--virtual-time-budget=8000"])
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg("--dump-dom")
        .arg(format!("file://{}", page.display()))
        .output()
        .expect("google-chrome runs");
    let dom = String::from_utf8_lossy(&out.stdout);
    let found = dom
        .split_once("<pre id=\"probe\">")
        .and_then(|(_, tail)| tail.split_once("</pre>"))
        .map(|(json, _)| json.replace("&quot;", "\"").replace("&amp;", "&"))
        .unwrap_or_else(|| panic!("the page did not answer the search:\n{dom}"));
    serde_json::from_str(&found).expect("the probe writes JSON")
}

/// No navegador sem janela, a busca por um código deixa só aquele item,
/// aberto e com o termo marcado, no grupo dele, aberto — mesmo com outros
/// itens citando o mesmo código. Precisa do `google-chrome`.
#[test]
#[ignore = "abre a página no google-chrome sem janela"]
fn a_search_for_a_code_leaves_only_that_item_open_and_marked_in_a_headless_browser() {
    let dir = tempfile::tempdir().expect("tempdir");
    let [(_, html), _] = generate(dir.path());
    assert!(html.matches("MSTD-RULE-0001").count() > 2, "other items cite the code");
    let page = dir.path().join("busca.html");
    fs::write(&page, html.replace("</body>", &format!("{}</body>", probe("MSTD-RULE-0001")))).expect("the copy");
    let found = browse(&page, &dir.path().join("perfil"));
    assert_eq!(found["items"], serde_json::json!(["MSTD-RULE-0001 open"]), "{found}");
    assert_eq!(found["marked"], serde_json::json!(["MSTD-RULE-0001"]), "{found}");
    assert_eq!(found["groups"], serde_json::json!(["agreed-rule open"]), "{found}");
    assert_eq!(found["hits"], "1 item", "{found}");

    // Só o fim do código também acha o item; uma palavra acha pelo texto,
    // sem acento e sem caixa.
    fs::write(&page, html.replace("</body>", &format!("{}</body>", probe("rule-0001")))).expect("the copy");
    assert_eq!(browse(&page, &dir.path().join("perfil"))["items"], serde_json::json!(["MSTD-RULE-0001 open"]));
    fs::write(&page, html.replace("</body>", &format!("{}</body>", probe("ANTIVIRUS")))).expect("the copy");
    let found = browse(&page, &dir.path().join("perfil"));
    assert_eq!(found["items"], serde_json::json!(["MSTD-DEFER-0001 open"]), "{found}");
    assert_eq!(found["marked"], serde_json::json!(["antivírus|antivírus"]), "{found}");
}
