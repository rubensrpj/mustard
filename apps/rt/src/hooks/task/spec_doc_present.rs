//! `spec_doc_present` — no fim de cada resposta em que o resumo legível da
//! unidade aberta (`resumo.html`) mudou, barra o fim com uma ordem ao
//! assistente: publicar a página no claude.ai, entregar o link ao usuário e
//! gravar o endereço.
//!
//! ## Por que existe
//!
//! A página que o `spec-doc` monta só serve se chegar ao usuário. Em 10/09/2026
//! ele recusou uma aprovação por não conseguir ler a spec no terminal (K-3) e
//! escolheu a entrega (K-7). No mesmo dia, diante da linha que oferecia a
//! publicação como opção dele, respondeu: "sempre tem que publicar, isso não é
//! opção". Publicar deixou de ser um pedido do usuário e virou parte da entrega.
//!
//! ## Os fatos, todos necessários
//!
//! 1. É o `Stop` da sessão principal — nunca o de um subagente — e não é a
//!    continuação que um bloqueio pediu (`stop_hook_active`).
//! 2. Há uma unidade aberta: `current_spec` nomeia uma, e o `meta.json` dela não
//!    diz `Completed` (o mesmo corte do `crystallise_nudge`, pelo mesmo motivo:
//!    o arquivo de estado de uma unidade fechada sobrevive até o `SessionEnd`).
//! 3. O `spec-doc` monta a página, e o hash dela difere do último que ESTE
//!    gancho mostrou. O marcador guarda o hash mostrado, não o gravado: uma
//!    página regravada à mão por `run spec-doc` ainda é cobrada uma vez.
//!
//! Com os três, o fim da resposta é barrado com a ordem.
//!
//! ## Por que um bloqueio, e não `systemMessage`
//!
//! RO-4.1, conferido em code.claude.com/docs/en/hooks.md em 10/09/2026:
//! `systemMessage` é "Warning message shown to the user" — chega ao usuário e
//! nunca ao modelo (E-1). Quem publica é o assistente, então a ordem tem de
//! chegar a ele, e no `Stop` o canal que chega é o bloqueio: um `Deny`, que o
//! `hook_output` escreve como `decision: block` com o motivo, e a conversa
//! continua com a ordem como próximo passo.
//!
//! ## Sem laço
//!
//! Duas garantias independentes (K-2). O marcador da versão é gravado ANTES de
//! barrar, então a mesma página nunca barra duas vezes. E a continuação que o
//! bloqueio pede chega com `stop_hook_active`, que solta sem remontar a página:
//! uma página que mude nessa continuação é cobrada no fim seguinte, porque o
//! marcador não a viu. O endereço gravado fica fora da página (ver `spec-doc`),
//! então gravá-lo não muda o hash nem pede outra publicação.
//!
//! ## O que a ordem diz
//!
//! Publicar `.claude/spec/<unidade>/resumo.html` no claude.ai — no MESMO
//! endereço quando `published-url` já guarda um, citado na ordem —, entregar o
//! link ao usuário numa linha própria e gravar o endereço com `run spec-doc`
//! e `--published-url`. Sem ferramenta de publicação, as formas de abrir de
//! sempre, conforme o lugar da sessão:
//!
//! - Sessão local: o link `file://`, que o terminal torna clicável.
//! - Sessão por SSH (`SSH_CONNECTION` ou `SSH_CLIENT`): o arquivo está no
//!   servidor e o navegador na outra ponta, onde `file://` não chega. Vão
//!   comandos `scp` prontos para colar — PowerShell do Windows, macOS e Linux —
//!   com o usuário de `$USER` e o host do terceiro campo de `SSH_CONNECTION` (o
//!   endereço do servidor).
//!
//! O texto sai do catálogo `i18n`, no idioma da spec e no tom do projeto; os
//! comandos e os endereços não se traduzem.
//! A origem do `scp` vai entre aspas simples, na regra de cada shell, para um
//! caminho com espaço seguir válido. Uma camada só de aspas, a do shell local:
//! o `scp` do OpenSSH 9 em diante usa SFTP e lê o caminho remoto literal.
//!
//! ## Abrir sozinho, só na espera de aprovação
//!
//! Estágio `Plan` sem `.approved-by-user` (o predicado compartilhado do
//! gravador de aprovação, mais o marcador), com tela local — `DISPLAY` ou
//! `WAYLAND_DISPLAY` no Linux; macOS e Windows sempre têm — e fora de SSH. Uma
//! vez por versão: o marcador de abertura é gravado ANTES de abrir, então um
//! marcador que não grava nunca vira uma aba nova por turno.
//! `MUSTARD_DOC_OPEN=off` desliga só a abertura; a ordem continua.
//!
//! ## Quando algo falha
//!
//! Spec ilegível, disco sem escrita, abridor ausente: o gancho só se cala neste
//! turno, e um marcador que não grava nunca barra. Limites conhecidos: se um
//! gancho irmão acima bloquear este mesmo `Stop`, o `Deny` dele vence o `fold`
//! e a ordem se perde com o marcador já gravado — a próxima mudança da página a
//! traz de volta; e, num fim barrado por esta ordem, a nota de clareza que o
//! `clarity_check`, registrado depois, levaria ao usuário não sai.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::io::fs;
use mustard_core::platform::error::Error;
use mustard_core::platform::i18n::I18n;
use mustard_core::ClaudePaths;

use crate::commands::spec::spec_doc::{generate, spec_i18n, SpecDocReport, DOC_FILE};
use crate::hooks::observe::approval_marker_observer::is_awaiting_approval;
use crate::hooks::observe::clarification_observer::spec_is_closed;
use crate::shared::context::{approval_marker_path, current_spec};

/// O interruptor da abertura automática do navegador.
const OPEN_ENV: &str = "MUSTARD_DOC_OPEN";

/// Se o navegador pode abrir sozinho na espera de aprovação.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenMode {
    /// Nunca abre; a ordem continua aparecendo.
    Off,
    /// Abre uma vez por versão (padrão).
    On,
}

/// Lê `MUSTARD_DOC_OPEN` (padrão `on`). Só `off` desliga; ausente ou qualquer
/// outro valor abre — é um conforto, não uma trava, então não há modo estrito.
fn open_mode() -> OpenMode {
    open_mode_from(std::env::var(OPEN_ENV).ok().as_deref())
}

fn open_mode_from(raw: Option<&str>) -> OpenMode {
    match raw.unwrap_or_default().trim().to_ascii_lowercase().as_str() {
        "off" => OpenMode::Off,
        _ => OpenMode::On,
    }
}

/// Onde a sessão roda — o que decide como o documento chega ao usuário quando
/// não há ferramenta de publicação.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Seat {
    /// Disco e terminal na mesma máquina; `display` diz se há tela para abrir.
    Local { display: bool },
    /// Sessão por SSH: o arquivo mora no servidor `host`, acessado como `user`.
    Remote { user: String, host: String },
}

impl Seat {
    fn detect() -> Self {
        Self::from_env(
            &|key| std::env::var(key).ok(),
            cfg!(any(target_os = "macos", target_os = "windows")),
        )
    }

    /// `native_display` é a tela que o sistema sempre tem (macOS, Windows); no
    /// Linux ela vem de `DISPLAY` / `WAYLAND_DISPLAY`. Sem o endereço do servidor
    /// (só `SSH_CLIENT`), vale `HOSTNAME`; sem nada, um marcador `HOST` que o
    /// `scp` recusa em voz alta em vez de copiar de outro lugar.
    fn from_env(env: &dyn Fn(&str) -> Option<String>, native_display: bool) -> Self {
        let var = |key: &str| env(key).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        let connection = var("SSH_CONNECTION");
        if connection.is_some() || var("SSH_CLIENT").is_some() {
            let host = connection
                .as_deref()
                .and_then(|c| c.split_whitespace().nth(2))
                .map(str::to_string)
                .or_else(|| var("HOSTNAME"))
                .unwrap_or_else(|| "HOST".to_string());
            // IPv6 vai entre colchetes, senão o `scp` lê o primeiro `:` como o
            // separador do caminho.
            let host = if host.contains(':') { format!("[{host}]") } else { host };
            let user = var("USER")
                .or_else(|| var("USERNAME"))
                .unwrap_or_else(|| "USER".to_string());
            return Self::Remote { user, host };
        }
        Self::Local {
            display: native_display || var("DISPLAY").is_some() || var("WAYLAND_DISPLAY").is_some(),
        }
    }

    fn can_open(&self) -> bool {
        matches!(self, Self::Local { display: true })
    }
}

/// O gancho de fim de turno que manda publicar o resumo da spec.
pub struct SpecDocPresent;

impl Check for SpecDocPresent {
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        // Fato 1 — o `Stop` da sessão principal. A continuação de um bloqueio
        // se solta em `order_verdict`, antes de a página ser remontada.
        if ctx.trigger != Some(Trigger::Stop) || input.is_subagent() {
            return Ok(Verdict::Allow);
        }
        let project_dir = ctx.project_dir_or_cwd(input);
        let root = Path::new(&project_dir);

        // Fato 2 — uma unidade aberta.
        let Some(spec) = current_spec(&project_dir).filter(|s| !s.is_empty()) else {
            return Ok(Verdict::Allow);
        };
        if spec_is_closed(root, &spec) {
            return Ok(Verdict::Allow);
        }

        // Fato 3 — a página mudou desde a última entrega.
        Ok(order_verdict(input, root, &spec, open_mode(), &Seat::detect(), &open_with_system))
    }
}

/// O veredito do fim de resposta da unidade `spec`. A continuação que um
/// bloqueio pediu (`stop_hook_active`) solta antes de remontar a página, então
/// não consome versão nenhuma (K-2); fora dela, a página que mudou barra o fim
/// com a ordem de publicar.
fn order_verdict(
    input: &HookInput,
    root: &Path,
    spec: &str,
    mode: OpenMode,
    seat: &Seat,
    opener: &dyn Fn(&Path) -> bool,
) -> Verdict {
    if stop_hook_active(input) {
        return Verdict::Allow;
    }
    present(root, spec, mode, seat, opener).map_or(Verdict::Allow, |reason| Verdict::Deny { reason })
}

/// `true` na continuação que um bloqueio do `Stop` pediu.
fn stop_hook_active(input: &HookInput) -> bool {
    input.raw.get("stop_hook_active").and_then(serde_json::Value::as_bool) == Some(true)
}

/// Monta a página e devolve a ordem ao assistente quando ela mudou desde a
/// última entrega; na espera de aprovação, abre-a por `opener` uma vez por
/// versão. `opener` é injetável para o teste nunca abrir um navegador de
/// verdade.
fn present(
    root: &Path,
    spec: &str,
    mode: OpenMode,
    seat: &Seat,
    opener: &dyn Fn(&Path) -> bool,
) -> Option<String> {
    let report = generate(root, spec);
    if !report.ok || report.hash.is_empty() {
        return None;
    }
    let file = root.join(&report.path);
    let project = root.to_string_lossy();
    let awaiting = is_awaiting_approval(&project, spec)
        && !approval_marker_path(&project, spec).is_some_and(|p| p.is_file());
    if awaiting
        && mode == OpenMode::On
        && seat.can_open()
        && remember(root, "opened", spec, &report.hash)
    {
        let _ = opener(&file);
    }
    if !remember(root, "shown", spec, &report.hash) {
        return None;
    }
    // O idioma e o tom são os da página: a ordem fala dela.
    let i18n = file.parent().map(|dir| spec_i18n(root, dir)).unwrap_or_default();
    Some(publish_order(awaiting, spec, &report, &file, seat, &i18n))
}

/// A ordem, uma instrução por linha: o que mudou; publicar, entregar o link e
/// gravar o endereço; republicar no endereço gravado, quando há um; e, para
/// quando não há ferramenta de publicação, cada forma de abrir.
fn publish_order(
    awaiting: bool,
    spec: &str,
    report: &SpecDocReport,
    file: &Path,
    seat: &Seat,
    i: &I18n,
) -> String {
    let head = if awaiting { "deliver.head.awaiting" } else { "deliver.head.summary" };
    let mut text = i.render(head).replace("{file}", DOC_FILE);
    let order = i.render("deliver.order").replace("{path}", &report.path).replace("{spec}", spec);
    let _ = write!(text, "\n{order}");
    if let Some(url) = report.published_url.as_deref() {
        let _ = write!(text, "\n{}", i.render("deliver.order.same").replace("{url}", url));
    }
    let _ = write!(text, "\n{}", i.render("deliver.fallback"));
    match seat {
        Seat::Local { .. } => {
            let _ = write!(text, "\n{}", i.render("deliver.click").replace("{url}", &report.url));
        }
        Seat::Remote { user, host } => {
            let source = format!("{user}@{host}:{}", file.display());
            let posix = posix_quote(&source);
            let commands = [
                (
                    "deliver.windows",
                    format!(
                        "scp {} $env:TEMP\\{DOC_FILE}; start $env:TEMP\\{DOC_FILE}",
                        powershell_quote(&source),
                    ),
                ),
                ("deliver.macos", format!("scp {posix} /tmp/{DOC_FILE} && open /tmp/{DOC_FILE}")),
                ("deliver.linux", format!("scp {posix} /tmp/{DOC_FILE} && xdg-open /tmp/{DOC_FILE}")),
            ];
            for (key, command) in commands {
                let _ = write!(text, "\n{}", i.render(key).replace("{command}", &command));
            }
        }
    }
    text
}

/// Aspas simples do PowerShell: o texto sai literal, e uma aspa simples dentro
/// dele se escreve dobrada.
fn powershell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// Aspas simples do sh (macOS, Linux): o texto sai literal; uma aspa simples
/// dentro dele fecha as aspas, entra escapada e as reabre.
fn posix_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// Grava `hash` como a última versão que `what` (`shown` / `opened`) viu desta
/// spec. `true` só quando a versão é nova E ficou gravada: um marcador que não
/// grava responde `false`, e o gancho se cala em vez de barrar a cada turno.
fn remember(root: &Path, what: &str, spec: &str, hash: &str) -> bool {
    let Some(path) = marker_path(root, what, spec) else {
        return false;
    };
    if fs::read_to_string(&path).is_ok_and(|seen| seen.trim() == hash) {
        return false;
    }
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write_atomic(&path, hash.as_bytes()).is_ok()
}

/// `<root>/.claude/.harness/spec-doc-<what>-<spec>`.
fn marker_path(root: &Path, what: &str, spec: &str) -> Option<PathBuf> {
    Some(
        ClaudePaths::for_project(root)
            .ok()?
            .harness_dir()
            .join(format!("spec-doc-{what}-{}", spec.replace(['/', '\\'], "-"))),
    )
}

/// O abridor do sistema (`open` / `start` / `xdg-open`). Nenhum fluxo é
/// herdado: o stdout do gancho é o JSON que o harness lê, e um filho segurando
/// o pipe prenderia a resposta até o navegador fechar.
fn open_with_system(file: &Path) -> bool {
    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut c = Command::new("cmd");
        c.args(["/C", "start", ""]);
        c
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = Command::new("xdg-open");
    command
        .arg(file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hook_output::hook_specific_output;
    use mustard_core::domain::model::contract::Outcome;
    use std::cell::RefCell;
    use tempfile::tempdir;

    const LOCAL: Seat = Seat::Local { display: true };

    /// Uma unidade `demo` no estágio pedido, com uma spec que dá para mudar.
    fn seed(root: &Path, stage: &str) {
        seed_in(root, stage, "pt-BR");
    }

    /// A mesma unidade, no idioma `lang` (projeto e spec).
    fn seed_in(root: &Path, stage: &str, lang: &str) {
        std::fs::write(root.join("mustard.json"), format!(r#"{{"specLang":"{lang}"}}"#)).unwrap();
        let dir = root.join(".claude/spec/demo");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("meta.json"),
            format!(r#"{{"stage":"{stage}","outcome":"Active","lang":"{lang}"}}"#),
        )
        .unwrap();
        rewrite_spec(root, "Primeira versão.");
    }

    /// O lugar de uma sessão por SSH no servidor `10.0.0.5`, como `rubens`.
    fn ssh_seat() -> Seat {
        Seat::from_env(
            &|key| match key {
                "SSH_CONNECTION" => Some("10.0.0.9 51234 10.0.0.5 22".to_string()),
                "USER" => Some("rubens".to_string()),
                "DISPLAY" => Some("localhost:10.0".to_string()),
                _ => None,
            },
            false,
        )
    }

    fn rewrite_spec(root: &Path, context: &str) {
        std::fs::write(
            root.join(".claude/spec/demo/spec.md"),
            format!("# Demo\n\n## Contexto\n\n{context}\n"),
        )
        .unwrap();
    }

    fn never(_: &Path) -> bool {
        panic!("only an approval wait on a local screen opens the browser")
    }

    /// Um fim de resposta comum da sessão principal.
    fn stop_input() -> HookInput {
        HookInput {
            hook_event_name: Some("Stop".to_string()),
            ..HookInput::default()
        }
    }

    /// A continuação que um bloqueio do `Stop` pediu.
    fn continuation() -> HookInput {
        let mut input = stop_input();
        input.raw = serde_json::json!({ "stop_hook_active": true });
        input
    }

    /// AC-1 — quando a página muda, o fim da resposta é barrado com uma ordem ao
    /// ASSISTENTE: publicar no claude.ai, entregar o link e gravar o endereço.
    /// Chega ao modelo como bloqueio, nunca como `systemMessage`, e não oferece
    /// mais a publicação ao usuário. Com um endereço gravado, a versão seguinte
    /// manda republicar nele.
    #[test]
    fn stop_orders_the_assistant_to_publish_the_changed_page() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        seed(root, "Execute");

        let verdict = order_verdict(&stop_input(), root, "demo", OpenMode::On, &LOCAL, &never);
        let Verdict::Deny { reason } = &verdict else {
            panic!("a changed page blocks the end of the answer: {verdict:?}");
        };
        for needle in [
            "publique .claude/spec/demo/resumo.html no claude.ai como página",
            "entregue o link ao usuário numa linha própria",
            "`mustard-rt run spec-doc --spec demo --published-url <endereço>`",
            "nunca é uma opção a oferecer ao usuário",
            "Sem ferramenta de publicação, entregue ao usuário as formas de abrir:",
            "- Clique: file://",
        ] {
            assert!(reason.contains(needle), "missing {needle}:\n{reason}");
        }
        assert!(!reason.contains("Peça ao assistente"), "publishing is not the user's option:\n{reason}");
        assert!(!reason.contains("MESMO endereço"), "nothing recorded, nothing to republish over:\n{reason}");

        // Chega ao modelo: `decision: block` com a ordem, sem `systemMessage`.
        let outcome = Outcome {
            verdict: Verdict::Deny {
                reason: reason.clone(),
            },
            warnings: Vec::new(),
        };
        let json = hook_specific_output("Stop", &outcome).expect("a Stop deny emits");
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["decision"].as_str(), Some("block"), "{json}");
        assert_eq!(parsed["reason"].as_str(), Some(reason.as_str()), "{json}");
        assert!(parsed.get("systemMessage").is_none(), "the order is not a note to the user: {json}");

        // A mesma página nunca barra duas vezes.
        assert_eq!(
            order_verdict(&stop_input(), root, "demo", OpenMode::On, &LOCAL, &never),
            Verdict::Allow,
        );

        // Com um endereço gravado, a versão seguinte manda republicar nele.
        let url = "https://claude.ai/code/artifacts/demo-page";
        std::fs::write(root.join(".claude/spec/demo/published-url"), format!("{url}\n")).unwrap();
        rewrite_spec(root, "Segunda versão.");
        let verdict = order_verdict(&stop_input(), root, "demo", OpenMode::On, &LOCAL, &never);
        let Verdict::Deny { reason } = &verdict else {
            panic!("the new version blocks again: {verdict:?}");
        };
        assert!(
            reason.contains(&format!("republique no MESMO endereço, {url}, e entregue esse link")),
            "{reason}",
        );
    }

    /// AC-2 — a continuação que a ordem pediu chega com `stop_hook_active` e não
    /// barra de novo; nem consome a versão, que um fim comum ainda cobra uma
    /// vez. Pelo `evaluate`, com a unidade aberta de verdade.
    #[test]
    fn publish_order_releases_on_stop_hook_active() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        seed(root, "Execute");
        // O arquivo de estado que `current_spec` lê.
        let states = root.join(".claude/.pipeline-states");
        std::fs::create_dir_all(&states).unwrap();
        std::fs::write(states.join("demo.json"), "{}").unwrap();
        let ctx = Ctx::for_test(root.to_string_lossy().into_owned(), Some(Trigger::Stop));

        // A página mudou, mas este fim é a continuação: solta.
        assert_eq!(SpecDocPresent.evaluate(&continuation(), &ctx).unwrap(), Verdict::Allow);
        assert_eq!(
            order_verdict(&continuation(), root, "demo", OpenMode::On, &LOCAL, &never),
            Verdict::Allow,
        );

        // A versão segue não vista: o fim comum seguinte barra com a ordem.
        assert!(SpecDocPresent.evaluate(&stop_input(), &ctx).unwrap().is_blocking());
        // A continuação dessa ordem solta, e a mesma versão não barra mais.
        assert_eq!(SpecDocPresent.evaluate(&continuation(), &ctx).unwrap(), Verdict::Allow);
        assert_eq!(SpecDocPresent.evaluate(&stop_input(), &ctx).unwrap(), Verdict::Allow);
    }

    /// AC-7 — a ordem aparece quando a página muda, e só então; sem ferramenta
    /// de publicação, o link local vai na mesma mensagem.
    #[test]
    fn stop_presents_doc_link_only_when_changed() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        seed(root, "Execute");

        let first = present(root, "demo", OpenMode::On, &LOCAL, &never).expect("first turn orders");
        assert!(first.contains("resumo da spec"), "{first}");
        assert!(first.contains("- Clique: file://") && first.contains("/resumo.html"), "{first}");
        assert!(first.contains("claude.ai"), "{first}");

        // A mesma página: silêncio, turno após turno.
        assert!(present(root, "demo", OpenMode::On, &LOCAL, &never).is_none());
        assert!(present(root, "demo", OpenMode::On, &LOCAL, &never).is_none());

        // A spec mudou: a página muda, e a ordem volta — uma vez.
        rewrite_spec(root, "Segunda versão.");
        assert!(present(root, "demo", OpenMode::On, &LOCAL, &never).is_some());
        assert!(present(root, "demo", OpenMode::On, &LOCAL, &never).is_none());
    }

    /// AC-8 — na espera de aprovação o documento abre uma vez por versão, e não
    /// abre com `MUSTARD_DOC_OPEN=off` nem depois de aprovado.
    #[test]
    fn approval_wait_opens_doc_once() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        seed(root, "Plan");
        let opened: RefCell<Vec<PathBuf>> = RefCell::new(Vec::new());
        let opener = |p: &Path| {
            opened.borrow_mut().push(p.to_path_buf());
            true
        };

        let message = present(root, "demo", OpenMode::On, &LOCAL, &opener).expect("shows");
        assert!(message.contains("spec para aprovar"), "{message}");
        assert_eq!(opened.borrow().len(), 1);
        assert!(opened.borrow()[0].ends_with(Path::new(".claude/spec/demo").join(DOC_FILE)));

        // A mesma versão nunca abre uma segunda aba.
        let _ = present(root, "demo", OpenMode::On, &LOCAL, &opener);
        assert_eq!(opened.borrow().len(), 1);

        // Versão nova: abre de novo, uma vez.
        rewrite_spec(root, "Segunda versão.");
        let _ = present(root, "demo", OpenMode::On, &LOCAL, &opener);
        let _ = present(root, "demo", OpenMode::On, &LOCAL, &opener);
        assert_eq!(opened.borrow().len(), 2);

        // `MUSTARD_DOC_OPEN=off`: nada abre, e a ordem continua.
        assert_eq!(open_mode_from(Some("off")), OpenMode::Off);
        assert_eq!(open_mode_from(Some(" OFF ")), OpenMode::Off);
        assert_eq!(open_mode_from(None), OpenMode::On);
        assert_eq!(open_mode_from(Some("on")), OpenMode::On);
        rewrite_spec(root, "Terceira versão.");
        assert!(present(root, "demo", open_mode_from(Some("off")), &LOCAL, &opener).is_some());
        assert_eq!(opened.borrow().len(), 2);

        // Aprovada, a spec não espera mais: nada abre.
        let marker = approval_marker_path(&root.to_string_lossy(), "demo").unwrap();
        std::fs::write(marker, "approved\n").unwrap();
        rewrite_spec(root, "Quarta versão.");
        let after = present(root, "demo", OpenMode::On, &LOCAL, &opener).expect("still shows");
        assert!(after.contains("resumo da spec"), "{after}");
        assert_eq!(opened.borrow().len(), 2);
    }

    /// Em SSH nada abre, mesmo esperando aprovação e com `DISPLAY`; a ordem de
    /// publicar vem primeiro, os `scp` prontos ficam para quando não há
    /// ferramenta de publicação, e nenhum `file://` aparece.
    #[test]
    fn ssh_session_never_opens_and_lists_the_copy_commands() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        seed(root, "Plan");
        let seat = ssh_seat();
        assert_eq!(seat, Seat::Remote { user: "rubens".to_string(), host: "10.0.0.5".to_string() });

        let message = present(root, "demo", OpenMode::On, &seat, &never).expect("shows");
        let source = format!("rubens@10.0.0.5:{}", root.join(".claude/spec/demo/resumo.html").display());
        for needle in [
            "no claude.ai como página".to_string(),
            "Sem ferramenta de publicação, entregue ao usuário as formas de abrir:".to_string(),
            format!(
                "- Windows, no PowerShell: scp '{source}' $env:TEMP\\resumo.html; start $env:TEMP\\resumo.html"
            ),
            format!("- macOS: scp '{source}' /tmp/resumo.html && open /tmp/resumo.html"),
            format!("- Linux: scp '{source}' /tmp/resumo.html && xdg-open /tmp/resumo.html"),
        ] {
            assert!(message.contains(&needle), "missing {needle}:\n{message}");
        }
        let order = message.find("no claude.ai").unwrap();
        let fallback = message.find("Sem ferramenta de publicação").unwrap();
        assert!(order < fallback, "publishing comes first, the copies are the fallback:\n{message}");
        assert!(!message.contains("file://"), "{message}");
        assert!(!message.contains("Peça ao assistente"), "{message}");
    }

    /// AC-11 — a ordem fala o idioma da spec: pt-BR num projeto que declara
    /// pt-BR, inglês num que declara en-US. Os comandos e o link não se
    /// traduzem.
    #[test]
    fn delivery_message_follows_spec_lang() {
        let pt = tempdir().unwrap();
        seed_in(pt.path(), "Plan", "pt-BR");
        let message = present(pt.path(), "demo", OpenMode::Off, &LOCAL, &never).expect("shows");
        for needle in [
            "Mustard · spec para aprovar: o resumo.html mudou.",
            "Antes de encerrar, publique .claude/spec/demo/resumo.html no claude.ai como página",
            "- Clique: file://",
        ] {
            assert!(message.contains(needle), "missing {needle}:\n{message}");
        }
        assert!(!message.contains("Before you finish"), "no English left in a pt-BR unit:\n{message}");

        let en = tempdir().unwrap();
        seed_in(en.path(), "Execute", "en-US");
        let message = present(en.path(), "demo", OpenMode::Off, &LOCAL, &never).expect("shows");
        for needle in [
            "Mustard · spec summary: resumo.html changed.",
            "Before you finish, publish .claude/spec/demo/resumo.html as a claude.ai page",
            "`mustard-rt run spec-doc --spec demo --published-url <url>`",
            "never an option to offer the user",
            "With no publishing tool, hand the user the ways to open it:",
            "- Click: file://",
        ] {
            assert!(message.contains(needle), "missing {needle}:\n{message}");
        }
        assert!(!message.contains("Ask the assistant"), "{message}");
    }

    /// AC-13 — um projeto num diretório com espaço ainda recebe um `scp` que
    /// funciona: a origem vai entre aspas simples, na regra de cada shell.
    #[test]
    fn scp_path_with_spaces_is_quoted() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("meu projeto");
        std::fs::create_dir_all(&root).unwrap();
        seed(&root, "Plan");

        let message = present(&root, "demo", OpenMode::On, &ssh_seat(), &never).expect("shows");
        let source = format!("rubens@10.0.0.5:{}", root.join(".claude/spec/demo/resumo.html").display());
        assert!(source.contains(' '), "the fixture path carries a space: {source}");
        for needle in [
            format!("scp '{source}' $env:TEMP\\resumo.html"),
            format!("scp '{source}' /tmp/resumo.html && open"),
            format!("scp '{source}' /tmp/resumo.html && xdg-open"),
        ] {
            assert!(message.contains(&needle), "missing {needle}:\n{message}");
        }
        assert!(!message.contains(&format!("scp {source}")), "an unquoted source breaks at the space");

        // Uma aspa simples no caminho também fica literal, em cada shell.
        assert_eq!(powershell_quote("a'b c"), "'a''b c'");
        assert_eq!(posix_quote("a'b c"), r"'a'\''b c'");
    }

    /// O lugar da sessão: SSH vence a tela; sem o endereço do servidor vale o
    /// `HOSTNAME`; IPv6 vai entre colchetes; no Linux sem tela nada abre.
    #[test]
    fn seat_follows_ssh_and_the_local_screen() {
        let from = |pairs: &[(&str, &str)], native: bool| {
            let owned: Vec<(String, String)> =
                pairs.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect();
            Seat::from_env(
                &|key| owned.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()),
                native,
            )
        };
        assert_eq!(
            from(&[("SSH_CLIENT", "10.0.0.9 51234 22"), ("HOSTNAME", "srv"), ("USER", "ana")], true),
            Seat::Remote { user: "ana".to_string(), host: "srv".to_string() },
        );
        assert_eq!(
            from(&[("SSH_CONNECTION", "fe80::9 51234 fe80::5 22"), ("USER", "ana")], false),
            Seat::Remote { user: "ana".to_string(), host: "[fe80::5]".to_string() },
        );
        assert!(!from(&[], false).can_open(), "a Linux box with no screen opens nothing");
        assert!(from(&[("WAYLAND_DISPLAY", "wayland-0")], false).can_open());
        assert!(from(&[], true).can_open(), "macOS and Windows always have a screen");
    }

    /// Fora do `Stop` da sessão principal o gancho nem olha a unidade.
    #[test]
    fn the_doc_link_self_restricts_to_the_main_stop() {
        let tmp = tempdir().unwrap();
        let project = tmp.path().to_string_lossy().into_owned();
        let ctx = |trigger| Ctx::for_test(project.clone(), Some(trigger));
        let sub = HookInput {
            hook_event_name: Some("Stop".to_string()),
            agent_id: Some("child".to_string()),
            ..HookInput::default()
        };
        assert_eq!(SpecDocPresent.evaluate(&sub, &ctx(Trigger::Stop)).unwrap(), Verdict::Allow);
        let pre = HookInput::default();
        assert_eq!(SpecDocPresent.evaluate(&pre, &ctx(Trigger::PreToolUse)).unwrap(), Verdict::Allow);
    }
}
