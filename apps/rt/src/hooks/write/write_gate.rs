//! `write_gate` — o portão de escrita.
//!
//! No `PreToolUse` das cinco ferramentas de arquivo — `Read`, `Write`, `Edit`,
//! `MultiEdit` e `NotebookEdit` —, o arquivo passa pelo classificador único
//! ([`WriteTarget::classify`]) e depois pelas regras ([`WriteRule`]), na ordem
//! de [`RULES`]. A primeira regra que responde decide; sem resposta, a
//! ferramenta passa.
//!
//! 1. **Segredo** ([`SecretRule`]): credenciais, chaves e a configuração do
//!    git não são lidas nem escritas, dentro ou fora do projeto.
//! 2. **Arquivos da spec** ([`SpecFileRule`]): o `spec.ndjson`, o `spec.md`, o
//!    `spec.html` e o `meta.json` da raiz de uma spec, o índice das specs e o
//!    banco de lições são gravados só pelo binário. A leitura passa.
//! 3. **Aprovação** ([`ApprovalRule`]): o código do projeto não muda enquanto
//!    a spec atual não está numa fase aprovada, pela mesma lista do `State`:
//!    em levantamento, em plano ou descartada; sem nenhum `state`, pela
//!    regra do [`lock_state`].
//! 4. **Branch da spec** ([`BranchRule`]): uma edição fora da branch em que a
//!    spec mora só avisa, nomeando as duas.
//! 5. **Base** ([`BaseRule`]): nenhuma edição direta numa base que o
//!    `git.flow` do `mustard.json` declara, fora dos planos e da evidência
//!    descartável. Sem `git.flow`, nenhuma branch é base; o portão nunca
//!    pergunta ao git qual é a branch padrão. Um `mustard.json` que existe e
//!    não se lê não é um projeto sem bases: ali o portão recusa toda escrita,
//!    menos a do próprio arquivo, que é como ele volta a se ler.
//!
//! O estado da spec vem do [`lock_state`], a regra única da trava: com algum
//! `state` no arquivo de eventos, vale a dobra deles; sem nenhum e com o
//! arquivo, a spec conta como em plano, e trava. Só o arquivo de eventos
//! conta: uma pasta de spec antiga, só com o `meta.json`, fica livre, e um
//! `meta.json` ao lado de um arquivo de eventos nunca muda o que o estado diz.
//! Livre também a branch que o Mustard não abriu, sem arquivo de eventos: ali
//! as regras da aprovação e da branch se calam. O portão não corta branch
//! nenhuma.
//!
//! ## Duas raízes
//!
//! Num worktree ligado, o despachante leva a raiz do projeto para o checkout
//! principal, onde moram o `mustard.json` e as specs. A branch, porém, é a da
//! árvore que recebe a edição ([`local_tree_of`]): a pasta do arquivo, senão a
//! pasta da sessão. Julgar pela branch do checkout principal barrava toda
//! edição feita num worktree de trabalho.
//!
//! ## Nunca falha
//!
//! O que não se lê — sem git, sem spec, arquivo ilegível — cala a regra que
//! dependia dele. O portão só barra com uma resposta positiva. A exceção é a
//! configuração do projeto: quando ela existe e não se lê, calar a regra da
//! base seria abrir a trava justo onde ela devia fechar, então ali a resposta
//! positiva é a própria leitura que falhou.
//!
//! ## Acrescentar uma regra
//!
//! Um tipo que implementa [`WriteRule`] e uma linha em [`RULES`], na posição
//! em que ela deve responder.

use std::collections::BTreeSet;
use std::path::Path;

use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::domain::spec_state::{SpecState, State};
use mustard_core::platform::error::Error;
use mustard_core::platform::i18n::{translate, Locale};

use crate::commands::git_settle::main_checkout_root;
use crate::shared::paths::{Access, PathClass, WriteTarget};
use crate::shared::spec_state::{lock_state, DiskSpecState};

/// O portão de escrita: todas as [`RULES`], e a primeira resposta vence.
pub struct WriteGate;

/// O que as regras sabem da edição, lido uma vez por chamada.
#[derive(Debug, Clone)]
pub(crate) struct WriteContext {
    /// A spec atual, pela escada única.
    pub(crate) spec: Option<String>,
    /// O estado da spec atual, pela trava ([`lock_state`]); `None` quando não
    /// há spec atual, ou numa branch que o Mustard não abriu.
    pub(crate) state: Option<State>,
    /// A branch da árvore que recebe a edição; `None` sem git ou com a
    /// cabeça solta.
    pub(crate) current_branch: Option<String>,
    /// A árvore que recebe a edição é o repositório do projeto ou um worktree
    /// dele. Um submódulo é outro repositório; na dúvida, `false`.
    pub(crate) in_project_repo: bool,
    /// As bases que o `git.flow` declara.
    pub(crate) bases: BTreeSet<String>,
    /// O `mustard.json` existe e não se lê: as bases acima estão vazias
    /// porque ninguém as leu, e não porque o projeto não declarou nenhuma.
    pub(crate) config_unreadable: bool,
    /// O idioma das mensagens.
    pub(crate) lang: Locale,
    /// Onde os testes começam numa leitura inteira que os tem dentro do
    /// arquivo ([`test_cut_line`]); `None` numa escrita, numa leitura que já
    /// pede um trecho, ou num arquivo sem a marca de uma linguagem conhecida.
    pub(crate) read_cut: Option<ReadCut>,
}

/// Onde a leitura inteira de um arquivo para, antes dos testes: o caminho
/// exatamente como a ferramenta o mandou — para o pedido reescrito continuar
/// válido — e a linha, contada a partir de 1, em que os testes começam.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReadCut {
    pub(crate) file_path: String,
    pub(crate) line: u64,
}

impl WriteContext {
    /// Lê o que as regras precisam para `target`, e só isso: a spec para uma
    /// escrita, e o estado e a branch para uma escrita num arquivo do projeto
    /// que não é do harness.
    fn read(root: &str, input: &HookInput, ctx: &Ctx, target: &WriteTarget) -> Self {
        let mut at = Self {
            spec: None,
            state: None,
            current_branch: None,
            in_project_repo: false,
            bases: ctx.config.git.declared_bases(),
            config_unreadable: ctx.config.unreadable,
            lang: ctx.config.language().text_or_default(),
            read_cut: test_cut_line(root, input, target),
        };
        if target.access != Access::Write {
            return at;
        }
        let disk = DiskSpecState::new(Path::new(root));
        at.spec = disk.active(input.session_id.as_deref());
        if !is_repo_work(&target.class) {
            return at;
        }
        at.state = at.spec.as_deref().and_then(|spec| lock_state(Path::new(root), spec));
        let tree = local_tree_of(input, root);
        at.current_branch = mustard_core::current_branch(Path::new(&tree));
        // Só a regra da aprovação pergunta, e só quando há estado e branch.
        at.in_project_repo =
            at.state.is_some() && at.current_branch.is_some() && same_repository(&tree, root);
        at
    }
}

/// `tree` é o repositório do projeto em `root` ou um worktree dele: os dois
/// têm o mesmo checkout principal. Um submódulo é outro repositório, com o
/// checkout principal dele. Na dúvida, `false`.
fn same_repository(tree: &str, root: &str) -> bool {
    let main = |dir: &str| {
        main_checkout_root(Path::new(dir)).map(|p| std::fs::canonicalize(&p).unwrap_or(p))
    };
    matches!((main(tree), main(root)), (Some(a), Some(b)) if a == b)
}

/// Uma regra do portão de escrita.
pub(crate) trait WriteRule {
    /// Julga a ferramenta sobre `target`. `None` quando a regra não tem o que
    /// dizer; uma regra nunca falha.
    fn judge(&self, target: &WriteTarget, at: &WriteContext) -> Option<Verdict>;
}

/// As regras, na ordem em que respondem. A leitura cortada vem por último:
/// um segredo, ou a spec, decide primeiro se a leitura passa.
pub(crate) const RULES: &[&dyn WriteRule] =
    &[&SecretRule, &SpecFileRule, &ApprovalRule, &BranchRule, &BaseRule, &ReadCutRule];

/// A marca que abre o módulo de testes de dentro do arquivo de código, pela
/// extensão do arquivo. Lugar único: uma linguagem nova entra numa linha,
/// sem mexer em [`test_cut_line`] nem em [`ReadCutRule`]. Onde o teste mora
/// num arquivo separado, nenhuma extensão bate, e a leitura passa inteira.
const TEST_MARKERS: &[(&str, &str)] = &[("rs", "#[cfg(test)]")];

/// A linha, contada a partir de 1, em que os testes começam — quando `input`
/// pede a leitura INTEIRA (sem `offset` nem `limit`) de um arquivo de código
/// do projeto cuja extensão está em [`TEST_MARKERS`] e cujo conteúdo tem a
/// marca. `None` numa leitura que já pede um trecho, numa escrita, num
/// arquivo fora do projeto ou sem a marca — nesses casos a leitura passa
/// como veio.
fn test_cut_line(root: &str, input: &HookInput, target: &WriteTarget) -> Option<ReadCut> {
    if target.access != Access::Read || target.class != PathClass::Production {
        return None;
    }
    let ti = &input.tool_input;
    if ti.get("offset").is_some() || ti.get("limit").is_some() {
        return None;
    }
    let extension = Path::new(&target.path).extension()?.to_str()?;
    let marker = TEST_MARKERS.iter().find(|(ext, _)| *ext == extension)?.1;
    let content = std::fs::read_to_string(Path::new(root).join(&target.path)).ok()?;
    let before = content.lines().position(|line| line.trim_start() == marker)?;
    if before == 0 {
        return None;
    }
    let file_path = input.file_path()?;
    Some(ReadCut { file_path, line: before as u64 + 1 })
}

impl Check for WriteGate {
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        Ok(run_rules(RULES, input, ctx))
    }
}

/// Roda `rules` sobre o `PreToolUse` de `input`: classifica o arquivo, lê o
/// contexto e devolve a primeira resposta.
pub(crate) fn run_rules(rules: &[&dyn WriteRule], input: &HookInput, ctx: &Ctx) -> Verdict {
    if ctx.trigger != Some(Trigger::PreToolUse) {
        return Verdict::Allow;
    }
    let root = ctx.project_dir_or_cwd(input);
    let Some(target) = WriteTarget::classify(&root, input) else {
        return Verdict::Allow;
    };
    let at = WriteContext::read(&root, input, ctx, &target);
    judge(rules, &target, &at)
}

/// A primeira resposta de `rules` para `target`; sem resposta, passa.
pub(crate) fn judge(rules: &[&dyn WriteRule], target: &WriteTarget, at: &WriteContext) -> Verdict {
    rules.iter().find_map(|rule| rule.judge(target, at)).unwrap_or(Verdict::Allow)
}

/// Um texto do catálogo com as vagas preenchidas.
pub(crate) fn say(key: &str, lang: Locale, slots: &[(&str, &str)]) -> String {
    slots
        .iter()
        .fold(translate(key, lang).to_string(), |text, (slot, value)| text.replace(slot, value))
}

/// Um arquivo do projeto que não é estado do harness: o que a branch protege.
fn is_repo_work(class: &PathClass) -> bool {
    matches!(class, PathClass::Production | PathClass::Artifact)
}

/// Um arquivo sensível não é lido nem escrito.
pub(crate) struct SecretRule;

impl WriteRule for SecretRule {
    fn judge(&self, target: &WriteTarget, at: &WriteContext) -> Option<Verdict> {
        let PathClass::Secret { pattern } = target.class else {
            return None;
        };
        let reason = say("write_gate.secret", at.lang, &[("{file}", &target.path), ("{pattern}", pattern)]);
        Some(Verdict::Deny { reason })
    }
}

/// Os arquivos que só o binário grava não são escritos à mão; a leitura passa.
pub(crate) struct SpecFileRule;

impl WriteRule for SpecFileRule {
    fn judge(&self, target: &WriteTarget, at: &WriteContext) -> Option<Verdict> {
        let PathClass::SpecFile { spec } = &target.class else {
            return None;
        };
        if target.access != Access::Write {
            return None;
        }
        let spec = spec.as_deref().or(at.spec.as_deref()).unwrap_or("<spec>");
        let reason = say("write_gate.spec_file", at.lang, &[("{file}", &target.path), ("{spec}", spec)]);
        Some(Verdict::Deny { reason })
    }
}

/// O código do projeto não muda antes de a spec atual ser aprovada. Numa
/// branch que o Mustard não abriu, ele não trava nada: com a branch da spec e
/// a atual conhecidas e diferentes, a atual fora das bases e a edição no
/// repositório do projeto ou num worktree dele, a regra se cala e só o aviso
/// da branch responde. Dentro de um submódulo, a trava continua: ali a
/// branch é a do submódulo, e a base dele pode não estar no `git.flow`.
pub(crate) struct ApprovalRule;

impl WriteRule for ApprovalRule {
    fn judge(&self, target: &WriteTarget, at: &WriteContext) -> Option<Verdict> {
        if target.access != Access::Write || target.class != PathClass::Production {
            return None;
        }
        let spec = at.spec.as_deref()?;
        let state = at.state.as_ref()?;
        if state.approved {
            return None;
        }
        if let (Some(home), Some(current)) = (state.branch.as_deref(), at.current_branch.as_deref())
            && home != current
            && !at.bases.contains(current)
            && at.in_project_repo
        {
            return None;
        }
        let reason = say("write_gate.not_approved", at.lang, &[("{spec}", spec), ("{file}", &target.path)]);
        Some(Verdict::Deny { reason })
    }
}

/// Uma edição fora da branch em que a spec mora só avisa: numa branch que o
/// Mustard não abriu, ele não trava nada.
pub(crate) struct BranchRule;

impl WriteRule for BranchRule {
    fn judge(&self, target: &WriteTarget, at: &WriteContext) -> Option<Verdict> {
        if target.access != Access::Write || !is_repo_work(&target.class) {
            return None;
        }
        let spec = at.spec.as_deref()?;
        let home = at.state.as_ref()?.branch.as_deref()?;
        let current = at.current_branch.as_deref()?;
        if current == home || at.bases.contains(current) {
            return None;
        }
        let message = say(
            "write_gate.other_branch",
            at.lang,
            &[("{spec}", spec), ("{branch}", home), ("{current}", current)],
        );
        Some(Verdict::Warn { message })
    }
}

/// Nenhuma edição direta numa base que o `git.flow` declara.
///
/// Um `mustard.json` que existe e não se lê não declara que nenhuma branch é
/// base: ele não declara nada, e ninguém sabe em que branch a edição está. Ali
/// a regra não libera, ela recusa toda escrita — menos a do próprio arquivo,
/// que é como ele volta a se ler.
pub(crate) struct BaseRule;

impl WriteRule for BaseRule {
    fn judge(&self, target: &WriteTarget, at: &WriteContext) -> Option<Verdict> {
        if target.access != Access::Write || !is_repo_work(&target.class) {
            return None;
        }
        // O caminho vem relativo à raiz, então a configuração do projeto é o
        // nome dela, sem pasta nenhuma na frente.
        if at.config_unreadable && target.path.trim() != "mustard.json" {
            let reason = say("write_gate.unreadable_config", at.lang, &[("{file}", &target.path)]);
            return Some(Verdict::Deny { reason });
        }
        let current = at.current_branch.as_deref().filter(|branch| at.bases.contains(*branch))?;
        let reason = say("write_gate.on_base", at.lang, &[("{branch}", current)]);
        Some(Verdict::Deny { reason })
    }
}

/// A leitura inteira de um arquivo de código com os testes dentro dele
/// ([`test_cut_line`]) para antes deles: o agente recebe só a produção, e um
/// aviso, nos dois idiomas, com a linha onde os testes começam e como pedir
/// esse trecho. Uma leitura que já pede um trecho, ou um arquivo sem a marca,
/// passa inteira — a regra corrige, nunca recusa.
pub(crate) struct ReadCutRule;

impl WriteRule for ReadCutRule {
    fn judge(&self, _target: &WriteTarget, at: &WriteContext) -> Option<Verdict> {
        let cut = at.read_cut.as_ref()?;
        let tool_input = serde_json::json!({ "file_path": cut.file_path, "limit": cut.line - 1 });
        let note = say("write_gate.read_cut", at.lang, &[("{line}", &cut.line.to_string())]);
        Some(Verdict::Rewrite { tool_input, note: Some(note) })
    }
}

/// A árvore que recebe a edição, onde a branch é lida: a pasta existente mais
/// próxima do arquivo, quando o caminho é absoluto; senão, a pasta da sessão;
/// senão, a raiz do projeto. Uma edição num worktree é julgada pela branch
/// do worktree, e não pela do checkout principal.
fn local_tree_of(input: &HookInput, root: &str) -> String {
    if let Some(file) = input.file_path() {
        let path = Path::new(&file);
        if path.is_absolute() {
            let mut dir = path.parent();
            while let Some(d) = dir {
                if d.is_dir() {
                    return d.to_string_lossy().into_owned();
                }
                dir = d.parent();
            }
        }
    }
    if let Some(cwd) = input.cwd.as_deref().filter(|c| !c.is_empty()) {
        return cwd.to_string();
    }
    root.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::context::session::bind_session_spec;
    use crate::shared::spec_state::stand_on_spec_branch;
    use mustard_core::io::spec_events as store;
    use mustard_core::ProjectConfig;
    use serde_json::{json, Value};
    use std::process::Command;

    /// As quatro ferramentas que escrevem.
    const WRITE_TOOLS: [&str; 4] = ["Write", "Edit", "MultiEdit", "NotebookEdit"];

    /// O fluxo deste repositório: `dev` e `main` são bases.
    const DEV_MAIN: &str = r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#;

    fn project(config: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("mustard.json"), config).expect("config");
        dir
    }

    /// O contexto que o despachante monta: a raiz e o `mustard.json` dela.
    fn ctx(root: &Path) -> Ctx {
        let mut ctx = Ctx::for_test(root.to_string_lossy().into_owned(), Some(Trigger::PreToolUse));
        ctx.config = ProjectConfig::load(root);
        ctx
    }

    fn lang(root: &Path) -> Locale {
        ProjectConfig::load(root).language().text_or_default()
    }

    /// A entrada de cada ferramenta, com o campo de caminho que ela usa.
    fn call(root: &Path, tool: &str, path: &str, session: Option<&str>) -> HookInput {
        let tool_input = match tool {
            "NotebookEdit" => json!({ "notebook_path": path, "new_source": "x" }),
            "MultiEdit" => json!({ "file_path": path, "edits": [{ "old_string": "a", "new_string": "b" }] }),
            "Edit" => json!({ "file_path": path, "old_string": "a", "new_string": "b" }),
            "Write" => json!({ "file_path": path, "content": "x" }),
            _ => json!({ "file_path": path }),
        };
        HookInput {
            tool_name: Some(tool.to_string()),
            tool_input,
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(root.to_string_lossy().into_owned()),
            session_id: session.map(str::to_string),
            ..HookInput::default()
        }
    }

    fn gate(root: &Path, tool: &str, path: &str) -> Verdict {
        WriteGate.evaluate(&call(root, tool, path, None), &ctx(root)).expect("never errors")
    }

    fn abs(root: &Path, rel: &str) -> String {
        root.join(rel).to_string_lossy().into_owned()
    }

    /// Grava um evento `state` na spec, pelo mesmo gravador do `run write`.
    fn record_state(root: &Path, spec: &str, fields: Value) -> u64 {
        let path = store::spec_file(root, spec).expect("spec file");
        std::fs::create_dir_all(path.parent().expect("spec folder")).expect("spec folder");
        store::write(&path, "state", fields.as_object().cloned().expect("object"), &[]).expect("state").id
    }

    fn approve(root: &Path, spec: &str) {
        record_state(
            root,
            spec,
            json!({ "phase": "approved", "witness": { "question": "Aprova?", "answer": "Aprovar" } }),
        );
    }

    fn git(root: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert!(ok, "git {args:?} failed");
    }

    /// Um repositório com um commit, parado em `branch`.
    fn repo_on(root: &Path, branch: &str) {
        git(root, &["init", "-q"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["checkout", "-q", "-b", branch]);
        std::fs::write(root.join("f.txt"), "hi").expect("file");
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", "init"]);
    }

    fn kind(verdict: &Verdict) -> &'static str {
        match verdict {
            Verdict::Allow => "allow",
            Verdict::Deny { .. } => "deny",
            Verdict::Warn { .. } => "warn",
            _ => "other",
        }
    }

    /// Escrever à mão num arquivo que só o binário grava é barrado por cada
    /// uma das quatro ferramentas de escrita, com a mensagem própria.
    #[test]
    fn every_write_tool_on_a_spec_file_is_blocked_with_its_own_message() {
        let dir = project("{}");
        let root = dir.path();
        let lang = lang(root);
        for (file, spec) in [
            (".claude/spec/x/spec.ndjson", "x"),
            (".claude/spec/x/spec.md", "x"),
            (".claude/spec/x/spec.html", "x"),
            (".claude/spec/x/meta.json", "x"),
            (".claude/spec/index.ndjson", "<spec>"),
            (".claude/spec/lessons.ndjson", "<spec>"),
        ] {
            let expected = say("write_gate.spec_file", lang, &[("{file}", file), ("{spec}", spec)]);
            for tool in WRITE_TOOLS {
                match gate(root, tool, &abs(root, file)) {
                    Verdict::Deny { reason } => assert_eq!(reason, expected, "{tool} {file}"),
                    other => panic!("{tool} on {file} must be refused, got {other:?}"),
                }
            }
        }
    }

    /// Com a spec atual em `plan`, o código do projeto é barrado pelas quatro
    /// ferramentas, com a mensagem própria; depois de "Aprovar", passa.
    #[test]
    fn a_production_edit_before_approval_is_blocked_with_its_own_message() {
        let dir = project("{}");
        let root = dir.path();
        stand_on_spec_branch(root, "x");
        record_state(root, "x", json!({ "phase": "plan", "branch": "feature/x" }));
        let expected = say("write_gate.not_approved", lang(root), &[("{spec}", "x"), ("{file}", "src/main.rs")]);
        for tool in WRITE_TOOLS {
            match gate(root, tool, &abs(root, "src/main.rs")) {
                Verdict::Deny { reason } => assert_eq!(reason, expected, "{tool}"),
                other => panic!("{tool} before approval must be refused, got {other:?}"),
            }
        }
        approve(root, "x");
        for tool in WRITE_TOOLS {
            assert_eq!(gate(root, tool, &abs(root, "src/main.rs")), Verdict::Allow, "{tool} after approval");
        }
    }

    /// Tirar o único `state` do arquivo não destrava a spec: sem `state` e sem
    /// `meta.json`, o arquivo de eventos conta como em plano.
    #[test]
    fn removing_the_only_state_keeps_the_approval_lock() {
        let dir = project("{}");
        let root = dir.path();
        stand_on_spec_branch(root, "x");
        let first = record_state(root, "x", json!({ "phase": "plan" }));
        let path = store::spec_file(root, "x").expect("spec file");
        let remove = json!({ "targets": [first], "reason": "engano" });
        store::write(&path, "remove", remove.as_object().cloned().expect("object"), &[]).expect("remove");
        assert!(
            matches!(gate(root, "Edit", &abs(root, "src/main.rs")), Verdict::Deny { .. }),
            "a spec file with no visible state is still not approved",
        );
    }

    /// Numa base declarada, as quatro ferramentas são barradas, com a mensagem
    /// própria; o que o harness gera em `.claude/` também é do repositório.
    #[test]
    fn an_edit_on_a_declared_base_is_blocked_with_its_own_message() {
        let dir = project(DEV_MAIN);
        let root = dir.path();
        repo_on(root, "dev");
        let expected = say("write_gate.on_base", lang(root), &[("{branch}", "dev")]);
        for tool in WRITE_TOOLS {
            for file in ["src/lib.rs", ".claude/settings.json"] {
                match gate(root, tool, &abs(root, file)) {
                    Verdict::Deny { reason } => assert_eq!(reason, expected, "{tool} {file}"),
                    other => panic!("{tool} on the base must be refused, got {other:?}"),
                }
            }
        }
    }

    /// Quebrar a configuração do projeto não abre a trava da base: parado na
    /// mesma branch de integração, com o arquivo válido o portão nega, e com o
    /// arquivo ilegível ele nega de novo, agora dizendo que a configuração não
    /// se lê. Só o próprio arquivo passa, que é como ele volta a se ler. Um
    /// projeto sem arquivo nenhum continua livre.
    #[test]
    fn a_broken_config_never_unlocks_the_base() {
        let dir = project(DEV_MAIN);
        let root = dir.path();
        repo_on(root, "dev");
        let on_base = say("write_gate.on_base", lang(root), &[("{branch}", "dev")]);
        match gate(root, "Edit", &abs(root, "src/lib.rs")) {
            Verdict::Deny { reason } => assert_eq!(reason, on_base, "the valid config refuses"),
            other => panic!("the base is refused, got {other:?}"),
        }

        std::fs::write(root.join("mustard.json"), "{ nao é json").expect("config");
        let expected = say("write_gate.unreadable_config", lang(root), &[("{file}", "src/lib.rs")]);
        for tool in WRITE_TOOLS {
            match gate(root, tool, &abs(root, "src/lib.rs")) {
                Verdict::Deny { reason } => assert_eq!(reason, expected, "{tool}"),
                other => panic!("{tool} with an unreadable config must be refused, got {other:?}"),
            }
        }
        assert_eq!(
            gate(root, "Write", &abs(root, "mustard.json")),
            Verdict::Allow,
            "the config itself is how it goes back to reading",
        );

        let empty = tempfile::tempdir().expect("tempdir");
        repo_on(empty.path(), "dev");
        assert_eq!(
            gate(empty.path(), "Edit", &abs(empty.path(), "src/lib.rs")),
            Verdict::Allow,
            "a project with no config declares no base",
        );
    }

    /// Fora da branch da spec, a edição só avisa, nomeando as duas branches;
    /// na branch da spec, passa; numa base, a regra da base responde.
    #[test]
    fn a_branch_other_than_the_spec_branch_only_warns() {
        let target = WriteTarget::classify("/p", &call(Path::new("/p"), "Edit", "/p/src/a.rs", None))
            .expect("a file tool");
        let at = |current: &str| WriteContext {
            spec: Some("x".to_string()),
            state: Some(State {
                phase: Some("running"),
                approved: true,
                branch: Some("feature/x".to_string()),
                ..State::default()
            }),
            current_branch: Some(current.to_string()),
            in_project_repo: true,
            bases: ["dev".to_string(), "main".to_string()].into(),
            config_unreadable: false,
            lang: Locale::PtBr,
            read_cut: None,
        };
        let warned = judge(RULES, &target, &at("feature/y"));
        assert_eq!(
            warned,
            Verdict::Warn {
                message: "[Mustard] A spec x mora na branch feature/x, e esta edição está na feature/y.".to_string()
            },
        );
        assert_eq!(judge(RULES, &target, &at("feature/x")), Verdict::Allow, "the spec's own branch");
        assert!(matches!(judge(RULES, &target, &at("dev")), Verdict::Deny { .. }), "a base is refused");
        let unopened = WriteContext { state: None, ..at("feature/y") };
        assert_eq!(judge(RULES, &target, &unopened), Verdict::Allow, "no event file, no warning");
    }

    /// Numa branch criada à mão, a aprovação não trava: com a spec em `plan`,
    /// só o aviso da branch sai. Na branch da spec, numa base ou com a branch
    /// desconhecida, a trava continua.
    #[test]
    fn a_hand_made_branch_is_never_trapped_by_the_approval() {
        let target = WriteTarget::classify("/p", &call(Path::new("/p"), "Edit", "/p/src/a.rs", None))
            .expect("a file tool");
        let at = |current: Option<&str>| WriteContext {
            spec: Some("x".to_string()),
            state: Some(State {
                phase: Some("plan"),
                branch: Some("feature/x".to_string()),
                ..State::default()
            }),
            current_branch: current.map(str::to_string),
            in_project_repo: true,
            bases: ["dev".to_string(), "main".to_string()].into(),
            config_unreadable: false,
            lang: Locale::PtBr,
            read_cut: None,
        };
        assert_eq!(
            judge(RULES, &target, &at(Some("minha-branch"))),
            Verdict::Warn {
                message: "[Mustard] A spec x mora na branch feature/x, e esta edição está na minha-branch."
                    .to_string()
            },
        );
        for (current, why) in [
            (Some("feature/x"), "the spec's own branch"),
            (Some("dev"), "a declared base"),
            (None, "an unknown branch"),
        ] {
            assert!(matches!(judge(RULES, &target, &at(current)), Verdict::Deny { .. }), "{why} keeps the lock");
        }
    }

    /// Num submódulo, a branch é a dele e a base dele pode faltar no
    /// `git.flow` da raiz: a edição ali, com a spec em plano, é barrada pela
    /// aprovação, mesmo com a branch do submódulo diferente da branch da spec.
    #[test]
    fn inside_a_submodule_the_approval_lock_stays() {
        let upstream = tempfile::tempdir().expect("tempdir");
        repo_on(upstream.path(), "master");
        let dir = project(DEV_MAIN);
        let root = dir.path();
        repo_on(root, "feature/x");
        record_state(root, "x", json!({ "phase": "plan", "branch": "feature/x" }));
        bind_session_spec(&root.to_string_lossy(), "s-sub", "x");
        let source = upstream.path().to_string_lossy().into_owned();
        git(root, &["-c", "protocol.file.allow=always", "submodule", "add", "-q", &source, "libs/sub"]);
        git(&root.join("libs").join("sub"), &["checkout", "-q", "-B", "master"]);
        let edit = |file: &str| {
            let input = call(root, "Write", &abs(root, file), Some("s-sub"));
            WriteGate.evaluate(&input, &ctx(root)).expect("never errors")
        };

        let expected = say(
            "write_gate.not_approved",
            lang(root),
            &[("{spec}", "x"), ("{file}", "libs/sub/a.rs")],
        );
        assert_eq!(edit("libs/sub/a.rs"), Verdict::Deny { reason: expected });

        // No repositório do projeto, uma branch feita à mão continua só avisando.
        git(root, &["checkout", "-q", "-b", "minha-branch"]);
        assert!(matches!(edit("src/a.rs"), Verdict::Warn { .. }), "{:?}", edit("src/a.rs"));
    }

    /// Uma spec cujo estado não diz a branch continua travada numa branch
    /// qualquer: sem a branch dela, nada prova que a edição está numa branch
    /// que o Mustard não abriu.
    #[test]
    fn a_spec_without_a_recorded_branch_keeps_the_approval_lock() {
        let dir = project(DEV_MAIN);
        let root = dir.path();
        repo_on(root, "minha-branch");
        record_state(root, "x", json!({ "phase": "plan" }));
        bind_session_spec(&root.to_string_lossy(), "s-no-branch", "x");
        let input = call(root, "Write", &abs(root, "src/a.rs"), Some("s-no-branch"));
        let expected = say("write_gate.not_approved", lang(root), &[("{spec}", "x"), ("{file}", "src/a.rs")]);
        assert_eq!(WriteGate.evaluate(&input, &ctx(root)).expect("never errors"), Verdict::Deny { reason: expected });
    }

    /// Ler um segredo é barrado como escrevê-lo; ler um arquivo da spec, o
    /// código antes da aprovação ou um arquivo numa base passa.
    #[test]
    fn reading_a_secret_is_blocked_and_reading_a_spec_file_passes() {
        let dir = project(DEV_MAIN);
        let root = dir.path();
        repo_on(root, "dev");
        bind_session_spec(&root.to_string_lossy(), "s-read", "x");
        record_state(root, "x", json!({ "phase": "plan" }));
        let read = |path: &str| {
            WriteGate.evaluate(&call(root, "Read", path, Some("s-read")), &ctx(root)).expect("never errors")
        };

        let key = "/home/u/.ssh/id_rsa";
        let expected = say("write_gate.secret", lang(root), &[("{file}", key), ("{pattern}", "id_rsa")]);
        assert_eq!(read(key), Verdict::Deny { reason: expected });
        assert!(matches!(gate(root, "Write", "config/credentials/prod.yaml"), Verdict::Deny { .. }));
        for path in [".claude/spec/x/spec.ndjson", ".claude/spec/lessons.ndjson", "src/lib.rs"] {
            assert_eq!(read(&abs(root, path)), Verdict::Allow, "reading {path} passes");
        }
    }

    /// A leitura inteira de um arquivo de código com o módulo de testes
    /// dentro dele para antes deles: o pedido reescrito pede só até a linha
    /// anterior, e o aviso, nos dois idiomas, nomeia a linha onde os testes
    /// começam.
    #[test]
    fn the_whole_read_of_a_code_file_stops_before_its_tests() {
        for lang in [Locale::PtBr, Locale::EnUs] {
            let cfg = format!(r#"{{"language":{{"text":"{}"}}}}"#, if lang == Locale::PtBr { "pt-BR" } else { "en-US" });
            let dir = project(&cfg);
            let root = dir.path();
            std::fs::create_dir_all(root.join("src")).unwrap();
            std::fs::write(root.join("src/a.rs"), "fn soma() {}\n\n#[cfg(test)]\nmod tests {}\n").unwrap();
            let path = abs(root, "src/a.rs");
            match WriteGate.evaluate(&call(root, "Read", &path, None), &ctx(root)).expect("never errors") {
                Verdict::Rewrite { tool_input, note } => {
                    assert_eq!(tool_input, json!({ "file_path": path, "limit": 2 }), "{lang:?}");
                    let note = note.expect("a note names the cut line");
                    assert!(note.contains('3'), "{lang:?}: {note}");
                }
                other => panic!("{lang:?}: the read is cut, got {other:?}"),
            }
        }
    }

    /// Uma leitura que já pede um trecho (`offset` ou `limit`) passa inteira,
    /// mesmo quando o arquivo tem o módulo de testes dentro dele.
    #[test]
    fn a_read_that_already_asks_for_an_excerpt_passes_whole() {
        let dir = project("{}");
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.rs"), "fn soma() {}\n\n#[cfg(test)]\nmod tests {}\n").unwrap();
        let path = abs(root, "src/a.rs");
        for tool_input in [json!({ "file_path": path, "offset": 1 }), json!({ "file_path": path, "limit": 10 })] {
            let input = HookInput {
                tool_name: Some("Read".to_string()),
                tool_input,
                hook_event_name: Some("PreToolUse".to_string()),
                cwd: Some(root.to_string_lossy().into_owned()),
                ..HookInput::default()
            };
            assert_eq!(WriteGate.evaluate(&input, &ctx(root)).expect("never errors"), Verdict::Allow);
        }
    }

    /// Um arquivo de código sem o módulo de testes dentro dele passa inteiro:
    /// a marca da linguagem não bate em lugar nenhum do conteúdo.
    #[test]
    fn a_code_file_with_no_tests_inside_passes_whole() {
        let dir = project("{}");
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.rs"), "fn soma() {}\n").unwrap();
        let path = abs(root, "src/a.rs");
        assert_eq!(gate(root, "Read", &path), Verdict::Allow);
    }

    /// Um projeto que declara `develop` e `master` no `git.flow`, num
    /// provedor Azure, tem as duas barradas; uma branch de trabalho passa.
    #[test]
    fn a_flow_with_develop_and_master_blocks_edits_on_both() {
        let dir = project(r#"{"git":{"flow":{"*":"develop","develop":"master"},"provider":"azure"}}"#);
        let root = dir.path();
        repo_on(root, "develop");
        for branch in ["develop", "master"] {
            if branch == "master" {
                git(root, &["checkout", "-q", "-b", "master"]);
            }
            let expected = say("write_gate.on_base", lang(root), &[("{branch}", branch)]);
            assert_eq!(gate(root, "Write", &abs(root, "src/a.rs")), Verdict::Deny { reason: expected });
        }
        git(root, &["checkout", "-q", "-b", "feature/x"]);
        assert_eq!(gate(root, "Write", &abs(root, "src/a.rs")), Verdict::Allow);
    }

    /// Sem `git.flow`, nenhuma branch é base: nem `main`, nem `master`. O
    /// portão não pergunta ao git qual é a branch padrão.
    #[test]
    fn without_a_declared_flow_no_branch_is_a_base() {
        for branch in ["main", "master"] {
            let dir = project("{}");
            let root = dir.path();
            repo_on(root, branch);
            assert_eq!(gate(root, "Edit", &abs(root, "f.txt")), Verdict::Allow, "{branch}");
        }
    }

    /// Uma pasta de spec antiga, só com o `meta.json` e sem o `spec.ndjson`,
    /// não trava, em qualquer estágio: a trava lê só o arquivo de eventos.
    #[test]
    fn a_spec_with_only_an_old_meta_json_does_not_lock() {
        for meta in [
            r#"{"scope":"light","stage":"Plan"}"#,
            r#"{"scope":"full","stage":"Analyze","outcome":"Active"}"#,
            r#"{"scope":"full","stage":"Execute","outcome":"Active"}"#,
            r#"{"scope":"light","stage":"Close","outcome":"Completed","phase":"CLOSE"}"#,
        ] {
            let dir = project("{}");
            let root = dir.path();
            stand_on_spec_branch(root, "x");
            std::fs::write(root.join(".claude").join("spec").join("x").join("meta.json"), meta).expect("meta");
            assert_eq!(gate(root, "Write", &abs(root, "src/main.rs")), Verdict::Allow, "{meta}");
            assert_eq!(lock_state(root, "x"), None, "{meta}");
        }
    }

    /// Uma spec aberta pelo `run write`, sem `meta.json`, conta como em plano
    /// e a trava fica fechada.
    #[test]
    fn a_spec_opened_by_run_write_stays_locked() {
        let dir = project("{}");
        let root = dir.path();
        stand_on_spec_branch(root, "x");
        let events = mustard_core::io::spec_events::spec_file(root, "x").expect("spec file");
        let note = json!({ "author": "user", "text": "um recado" });
        mustard_core::io::spec_events::write(&events, "message", note.as_object().cloned().unwrap(), &[])
            .expect("message");
        let locked = |root: &Path| matches!(gate(root, "Write", &abs(root, "src/main.rs")), Verdict::Deny { .. });
        assert!(locked(root), "a note alone counts as a plan");

        assert!(!root.join(".claude").join("spec").join("x").join("meta.json").exists(), "no meta.json is born");
        assert!(locked(root), "still locked");
        assert_eq!(lock_state(root, "x").and_then(|state| state.phase), Some("plan"), "still in plan");
    }

    /// Um estado sempre vence um `meta.json` posto ao lado: uma spec aberta
    /// pelo `open`, em levantamento, com um `meta.json` em execução ou
    /// encerrada, continua travada.
    #[test]
    fn a_state_always_wins_over_a_stray_meta_json() {
        for meta in [
            r#"{"scope":"light","stage":"Execute","outcome":"Active"}"#,
            r#"{"scope":"light","stage":"Close","outcome":"Completed"}"#,
        ] {
            let dir = project("{}");
            let root = dir.path();
            stand_on_spec_branch(root, "x");
            assert_eq!(crate::commands::spec_events::write::record_open(root, "x", "feature/x", "dev"), Ok(true));
            std::fs::write(root.join(".claude").join("spec").join("x").join("meta.json"), meta).expect("meta");
            assert!(matches!(gate(root, "Write", &abs(root, "src/main.rs")), Verdict::Deny { .. }), "{meta}");
            assert_eq!(lock_state(root, "x").and_then(|state| state.phase), Some("survey"), "{meta}");
        }
    }

    /// Uma pasta de spec sem `spec.ndjson` e sem `meta.json` é uma branch que
    /// o Mustard não abriu: nem a aprovação nem a branch da spec travam nada
    /// nela.
    #[test]
    fn a_branch_the_mustard_did_not_open_is_never_trapped() {
        let dir = project(DEV_MAIN);
        let root = dir.path();
        repo_on(root, "feature/outra");
        std::fs::create_dir_all(root.join(".claude").join("spec").join("x")).expect("spec folder");
        bind_session_spec(&root.to_string_lossy(), "s-unopened", "x");
        let input = call(root, "Write", &abs(root, "src/a.rs"), Some("s-unopened"));
        assert_eq!(WriteGate.evaluate(&input, &ctx(root)).expect("never errors"), Verdict::Allow);
    }

    /// Num worktree ligado, a raiz do projeto é o checkout principal, mas a
    /// branch é a do worktree: a edição no worktree de trabalho passa, e a do
    /// checkout principal, parado na base, é barrada. Vista do worktree, a
    /// spec do checkout principal continua gravada só pelo binário.
    #[test]
    fn an_edit_inside_a_linked_worktree_is_judged_by_the_worktree_branch() {
        let tmp = tempfile::tempdir().expect("tempdir");
        // No macOS a pasta temporária é um atalho (`/var` aponta para
        // `/private/var`), e o portão compara caminhos já resolvidos.
        // No Windows o caminho resolvido volta com o prefixo `\\?\`, que o
        // classificador não usa; tirá-lo deixa a comparação igual nos dois.
        let tmp_root = std::fs::canonicalize(tmp.path()).expect("tempdir resolvida");
        let tmp_root = std::path::PathBuf::from(
            tmp_root.to_string_lossy().trim_start_matches(r"\\?\").to_string(),
        );
        let main = tmp_root.join("repo");
        std::fs::create_dir_all(&main).expect("main");
        std::fs::write(main.join("mustard.json"), DEV_MAIN).expect("config");
        repo_on(&main, "dev");
        git(&main, &["worktree", "add", "-q", ".claude/worktrees/dev_x", "-b", "dev_x"]);
        let wt = main.join(".claude").join("worktrees").join("dev_x");

        let in_worktree = HookInput {
            cwd: Some(wt.to_string_lossy().into_owned()),
            ..call(&main, "Write", &abs(&wt, "f.txt"), None)
        };
        assert_eq!(WriteGate.evaluate(&in_worktree, &ctx(&main)).expect("never errors"), Verdict::Allow);
        assert!(matches!(gate(&main, "Write", &abs(&main, "f.txt")), Verdict::Deny { .. }));

        let spec_md = abs(&main, ".claude/spec/x/spec.md");
        let from_worktree = WriteGate.evaluate(&call(&wt, "Edit", &spec_md, None), &ctx(&wt));
        match from_worktree.expect("never errors") {
            Verdict::Deny { reason } => assert!(reason.contains(".claude/spec/x/spec.md"), "{reason}"),
            other => panic!("the main checkout's spec is refused from the worktree, got {other:?}"),
        }
    }

    /// Numa base, os planos e a evidência descartável seguem graváveis, e o
    /// que fica fora do projeto também; um arquivo que só cita `scratch` no
    /// nome é do repositório.
    #[test]
    fn plans_and_scratch_stay_writable_on_a_base() {
        let dir = project(DEV_MAIN);
        let root = dir.path();
        repo_on(root, "dev");
        for file in [".claude/plans/plano.md", ".claude/scratch/probe.sh", ".claude/scratch/data/case.json"] {
            assert_eq!(gate(root, "Write", &abs(root, file)), Verdict::Allow, "{file}");
        }
        let outside = tempfile::tempdir().expect("tempdir");
        assert_eq!(gate(root, "Write", &abs(outside.path(), "memo.md")), Verdict::Allow);
        assert!(matches!(gate(root, "Write", &abs(root, "src/scratch_notes.rs")), Verdict::Deny { .. }));
    }

    /// Fora do `PreToolUse`, e numa ferramenta que não é de arquivo, nada é
    /// julgado.
    #[test]
    fn only_the_pre_tool_use_of_a_file_tool_is_judged() {
        let dir = project(DEV_MAIN);
        let root = dir.path();
        let secret = call(root, "Read", "/p/cert.pem", None);
        let mut post = ctx(root);
        post.trigger = Some(Trigger::PostToolUse);
        assert_eq!(WriteGate.evaluate(&secret, &post).expect("never errors"), Verdict::Allow);
        let bash = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": "cat server.pem" }),
            ..HookInput::default()
        };
        assert_eq!(WriteGate.evaluate(&bash, &ctx(root)).expect("never errors"), Verdict::Allow);
    }

    // --- As tabelas dos três ganchos que o portão juntou ---------------------

    /// A tabela dos segredos: cada caminho sensível é barrado, sem distinguir
    /// maiúsculas e sobre o caminho inteiro, e o que não é segredo passa.
    #[test]
    fn the_secret_table_is_kept() {
        let dir = project("{}");
        let root = dir.path();
        for (tool, path, expected) in [
            ("Read", "/project/secrets/server.pem", "deny"),
            ("Write", "config/private.key", "deny"),
            ("Read", "/project/.aws/credentials", "deny"),
            ("Edit", "/project/.git/config", "deny"),
            ("Read", "/home/user/.ssh/id_rsa", "deny"),
            ("Read", "/home/user/.ssh/id_ed25519", "deny"),
            ("Read", "/p/cert.pfx", "deny"),
            ("Read", "/p/cert.p12", "deny"),
            ("Read", "x/Credentials.json", "deny"),
            ("Read", "certs/KEY.PEM", "deny"),
            ("Write", "config/credentials/prod.yaml", "deny"),
            ("Edit", "backup/ID_RSA.bak", "deny"),
            ("MultiEdit", "deploy/server.key", "deny"),
            ("NotebookEdit", "notes/credentials.ipynb", "deny"),
            ("Read", "/project/.env", "allow"),
            ("Write", "/project/.env.local", "allow"),
            ("Edit", "/project/src/main.ts", "allow"),
        ] {
            assert_eq!(kind(&gate(root, tool, path)), expected, "{tool} {path}");
        }
    }

    /// A janela da aprovação, lida do `State`: em levantamento e em plano o
    /// código é barrado e o que é do harness passa; aprovada, em execução ou
    /// sem spec atual, passa; a spec que vem só da ligação da sessão barra do
    /// mesmo jeito.
    #[test]
    fn the_approval_window_is_read_from_the_state() {
        let write = |root: &Path, file: &str, session: Option<&str>| {
            let input = call(root, "Write", &abs(root, file), session);
            kind(&WriteGate.evaluate(&input, &ctx(root)).expect("never errors"))
        };

        for phase in ["survey", "plan"] {
            let dir = project("{}");
            stand_on_spec_branch(dir.path(), "epic");
            record_state(dir.path(), "epic", json!({ "phase": phase }));
            assert_eq!(write(dir.path(), "src/main.rs", None), "deny", "{phase}");
            assert_eq!(write(dir.path(), ".claude/settings.json", None), "allow", "{phase}");
        }

        let approved = project("{}");
        stand_on_spec_branch(approved.path(), "epic");
        record_state(approved.path(), "epic", json!({ "phase": "plan" }));
        approve(approved.path(), "epic");
        assert_eq!(write(approved.path(), "src/main.rs", None), "allow");

        let running = project("{}");
        stand_on_spec_branch(running.path(), "epic");
        record_state(running.path(), "epic", json!({ "phase": "plan" }));
        approve(running.path(), "epic");
        record_state(running.path(), "epic", json!({ "phase": "running" }));
        assert_eq!(write(running.path(), "src/main.rs", None), "allow");

        let none = project("{}");
        assert_eq!(write(none.path(), "src/main.rs", None), "allow");

        let bound = project("{}");
        record_state(bound.path(), "epic", json!({ "phase": "plan" }));
        bind_session_spec(&bound.path().to_string_lossy(), "sess-1", "epic");
        assert_eq!(write(bound.path(), "src/main.rs", Some("sess-1")), "deny");
    }

    /// Sem marca de branch pendente, o que o gancho da branch fazia continua:
    /// a base barra, os planos, a evidência e o que fica fora do projeto
    /// passam, a branch de trabalho edita livre, e sem git nada é julgado.
    #[test]
    fn the_bare_base_table_is_kept() {
        let dir = project(DEV_MAIN);
        let root = dir.path();
        repo_on(root, "dev");
        let outside = tempfile::tempdir().expect("tempdir");
        for (path, expected) in [
            (abs(root, "f.txt"), "deny"),
            (abs(root, ".claude/plans/my-plan.md"), "allow"),
            (abs(root, ".claude/scratch/probe.sh"), "allow"),
            (abs(root, "src/scratch_notes.rs"), "deny"),
            (abs(outside.path(), "memo.md"), "allow"),
        ] {
            assert_eq!(kind(&gate(root, "Write", &path)), expected, "{path}");
        }

        let work = project(DEV_MAIN);
        repo_on(work.path(), "dev_thing");
        assert_eq!(gate(work.path(), "Write", &abs(work.path(), "f.txt")), Verdict::Allow);

        let bare = tempfile::tempdir().expect("tempdir");
        assert_eq!(gate(bare.path(), "Write", &abs(bare.path(), "f.txt")), Verdict::Allow);
    }
}
