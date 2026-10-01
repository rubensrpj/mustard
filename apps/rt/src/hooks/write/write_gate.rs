//! `write_gate` — o portão de escrita.
//!
//! No `PreToolUse` das cinco ferramentas de arquivo — `Read`, `Write`, `Edit`,
//! `MultiEdit` e `NotebookEdit` —, o arquivo passa pelo classificador único
//! ([`WriteTarget::classify`]) e depois pelas regras ([`WriteRule`]), na ordem
//! de [`RULES`]. A primeira regra que responde decide; sem resposta, a
//! ferramenta passa. A busca (`Grep`) não tem um arquivo só e responde à
//! parte, em [`search_verdict`].
//!
//! 1. **Segredo** ([`SecretRule`]): credenciais, chaves e a configuração do
//!    git não são lidas nem escritas, dentro ou fora do projeto.
//! 2. **Chave do Jev** ([`ConfigKeyRule`]): a leitura do `mustard.json` que
//!    guarda a chave volta com o arquivo, e a chave trocada por `***`.
//! 3. **Arquivos da spec** ([`SpecFileRule`]): o `spec.ndjson`, o `spec.md`, o
//!    `spec.html` e o `meta.json` da raiz de uma spec, o índice das specs e o
//!    banco de lições são gravados só pelo binário. A leitura passa.
//! 4. **Aprovação** ([`ApprovalRule`]): o código do projeto não muda enquanto
//!    a spec atual não está numa fase aprovada, pela mesma lista do `State`:
//!    em levantamento, em plano ou descartada; sem nenhum `state`, pela
//!    regra do [`lock_state`].
//! 5. **Branch da spec** ([`BranchRule`]): uma edição fora da branch em que a
//!    spec mora só avisa, nomeando as duas.
//! 6. **Base** ([`BaseRule`]): nenhuma edição direta numa base que o
//!    `git.flow` do `mustard.json` declara, fora dos planos e da evidência
//!    descartável. Sem `git.flow`, nenhuma branch é base; o portão nunca
//!    pergunta ao git qual é a branch padrão. Um `mustard.json` que existe e
//!    não se lê não é um projeto sem bases: ali o portão recusa toda escrita,
//!    menos a do próprio arquivo, que é como ele volta a se ler.
//! 7. **Leitura inteira grande** ([`WholeReadRule`]): a leitura inteira de um
//!    arquivo de código do mapa que traria mais de 300 linhas volta com as
//!    partes dele e o comando que traz só a parte certa.
//! 8. **Leitura cortada** ([`ReadCutRule`]): a leitura inteira de um arquivo
//!    com os testes dentro dele para antes deles.
//!
//! As regras da leitura valem também para o arquivo de uma cópia de trabalho
//! ligada ao mesmo repositório, como a cópia de uma onda: o caminho dela se
//! lê como o do projeto.
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
use mustard_core::platform::i18n::Locale;

use crate::commands::git_settle::main_checkout_root;
use crate::commands::spec_events::conversation::record_measured_call;
use crate::shared::code_route::{self, ProjectPath};
use crate::shared::config_key;
use crate::shared::paths::{Access, PathClass, WriteTarget};
use crate::shared::word_search;
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
    /// A recusa da leitura inteira de um arquivo de código do mapa que traria
    /// mais linhas que o teto ([`code_route::whole_read`]); `None` fora da
    /// leitura inteira, no arquivo pequeno e no que o mapa não guarda.
    pub(crate) whole_read: Option<String>,
    /// A recusa da leitura do `mustard.json` que guarda a chave do Jev, com o
    /// arquivo sem a chave ([`config_key::refusal`]); `None` em todo o resto.
    pub(crate) config_key: Option<String>,
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
        let lang = ctx.config.language().text_or_default();
        let whole = whole_read_of(root, input, target);
        let read_cut = whole.as_ref().and_then(|(file, content)| test_cut_line(file, content, input));
        let whole_read = whole.as_ref().and_then(|(file, content)| {
            let lines = read_cut.as_ref().map_or_else(|| content.lines().count(), |cut| cut.line as usize - 1);
            code_route::whole_read(Path::new(root), file, content, lines, lang)
        });
        let config_key = (target.access == Access::Read && config_key::is_config_file(&target.path))
            .then(|| input.file_path())
            .flatten()
            .and_then(|given| config_key::refusal(&target.path, &Path::new(root).join(given), lang));
        let mut at = Self {
            spec: None,
            state: None,
            current_branch: None,
            in_project_repo: false,
            bases: ctx.config.git.declared_bases(),
            config_unreadable: ctx.config.unreadable,
            lang,
            read_cut,
            whole_read,
            config_key,
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

/// As regras, na ordem em que respondem. As da leitura inteira vêm por
/// último: um segredo, a chave ou a spec decidem primeiro se a leitura passa,
/// e a leitura grande volta antes de ser cortada.
pub(crate) const RULES: &[&dyn WriteRule] = &[
    &SecretRule,
    &ConfigKeyRule,
    &SpecFileRule,
    &ApprovalRule,
    &BranchRule,
    &BaseRule,
    &WholeReadRule,
    &ReadCutRule,
];

/// A marca que abre o módulo de testes de dentro do arquivo de código, pela
/// extensão do arquivo. Lugar único: uma linguagem nova entra numa linha,
/// sem mexer em [`test_cut_line`] nem em [`ReadCutRule`]. Onde o teste mora
/// num arquivo separado, nenhuma extensão bate, e a leitura passa inteira.
const TEST_MARKERS: &[(&str, &str)] = &[("rs", "#[cfg(test)]")];

/// O arquivo e o conteúdo de uma leitura INTEIRA (sem `offset` nem `limit`)
/// de um arquivo de código do projeto: na raiz, ou numa cópia de trabalho
/// ligada ao mesmo repositório, onde ele se classifica a partir da raiz da
/// cópia. `None` numa escrita, numa leitura que já pede um trecho, num
/// arquivo que não é código do projeto ou que não se lê.
fn whole_read_of(root: &str, input: &HookInput, target: &WriteTarget) -> Option<(ProjectPath, String)> {
    if target.access != Access::Read {
        return None;
    }
    let ti = &input.tool_input;
    if ti.get("offset").is_some() || ti.get("limit").is_some() {
        return None;
    }
    let file = match target.class {
        PathClass::Production => ProjectPath {
            tree: Path::new(root).to_path_buf(),
            rel: target.path.clone(),
            abs: Path::new(root).join(&target.path),
        },
        PathClass::OutsideRepo => {
            let file = code_route::project_path(root, root, &input.file_path()?)?;
            let inner = WriteTarget::classify(&file.tree.to_string_lossy(), input)?;
            if inner.class != PathClass::Production {
                return None;
            }
            file
        }
        _ => return None,
    };
    let content = std::fs::read_to_string(&file.abs).ok()?;
    Some((file, content))
}

/// A linha, contada a partir de 1, em que os testes começam na leitura
/// inteira de `file`, com o conteúdo `content`, quando a extensão dele está
/// em [`TEST_MARKERS`] e o conteúdo tem a marca. `None` sem a marca — a
/// leitura então passa como veio.
fn test_cut_line(file: &ProjectPath, content: &str, input: &HookInput) -> Option<ReadCut> {
    let extension = Path::new(&file.rel).extension()?.to_str()?;
    let marker = TEST_MARKERS.iter().find(|(ext, _)| *ext == extension)?.1;
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
    match input.tool_name.as_deref() {
        Some("Grep") => return search_verdict(&root, input, ctx),
        Some("Glob") => return glob_verdict(&root, input, ctx),
        _ => {}
    }
    let Some(target) = WriteTarget::classify(&root, input) else {
        return Verdict::Allow;
    };
    let at = WriteContext::read(&root, input, ctx, &target);
    judge(rules, &target, &at)
}

/// A busca (`Grep`): no `mustard.json` que guarda a chave do Jev, a recusa
/// com o arquivo sem a chave; numa pasta de código do projeto — a raiz, sem
/// `path`, ou uma cópia de trabalho dele —, a resposta do mapa
/// ([`word_search`]): com o mapa cravado, a busca que traz as linhas
/// (`output_mode` `content`) é recusada com a resposta agrupada por função no
/// lugar dela; com o parcial, a busca roda, com a nota do mapa junto, só como
/// contexto (o parcial passa antes pelo filtro do mapa, que entrega só as
/// peças certas, e sem chave ou com o filtro falhando vale a triagem, com o
/// aviso uma vez por sessão); a que só lista arquivos ou conta segue, sem
/// filtro e com uma linha da marca; sem achado, ou com o filtro dizendo que
/// nada serve, a busca segue com uma linha do que o mapa não achou. A busca num arquivo só, fora do projeto,
/// com um `glob` que deixa só arquivos fora do mapa — pelos filtros de entrada
/// ou pelos de saída (`!*.rs`) — ou com a chave `search.answer` desligada
/// passa. A busca que traz as linhas numa pasta que guarda o arquivo com a
/// chave, com um `glob` que casa com o nome dele e passa por cima do que o
/// git ignora, é recusada também.
fn search_verdict(root: &str, input: &HookInput, ctx: &Ctx) -> Verdict {
    let lang = ctx.config.language().text_or_default();
    let ti = &input.tool_input;
    let text = |field: &str| ti.get(field).and_then(serde_json::Value::as_str).filter(|value| !value.trim().is_empty());
    let flag = |field: &str| ti.get(field).and_then(serde_json::Value::as_bool).unwrap_or(false);
    let base = input.cwd.as_deref().filter(|cwd| !cwd.is_empty()).unwrap_or(root);
    let path = text("path");
    if let Some(path) = path.filter(|path| config_key::is_config_file(path)) {
        return match config_key::refusal(path, &Path::new(base).join(path), lang) {
            Some(reason) => Verdict::Deny { reason },
            None => Verdict::Allow,
        };
    }
    let Some(pattern) = text("pattern") else { return Verdict::Allow };
    let (mut filters, typed) = word_search::tool_filters(text("glob"), text("type"));
    let walk = config_key::Walk::Rg { unignored: false };
    let answered = match (typed, code_route::project_path(root, base, path.unwrap_or(".")).filter(|folder| folder.abs.is_dir())) {
        (Some(typed), Some(folder)) if !flag("multiline") => {
            filters.extend(typed);
            let patterns = [pattern.to_string()];
            let search = word_search::Search {
                patterns: &patterns,
                dialect: word_search::Dialect::Rust,
                ignore_case: flag("-i"),
                whole_word: false,
                folders: std::slice::from_ref(&folder),
                filters: &filters,
                walk,
                shows_lines: text("output_mode") == Some("content"),
            };
            search_reply(root, input, ctx, &search)
        }
        _ => word_search::Reply::Pass,
    };
    if let word_search::Reply::Answer(reason) = answered {
        return Verdict::Deny { reason };
    }
    // Só o modo que traz as linhas mostraria a chave; os outros listam
    // arquivos ou contam.
    if text("output_mode") == Some("content") {
        let folder = Path::new(base).join(path.unwrap_or("."));
        if let Some(file) = config_key::swept(&[folder], Path::new(root), walk, &filters) {
            return Verdict::Deny { reason: say("config_key.swept_tool", lang, &[("{file}", &file)]) };
        }
    }
    match answered {
        word_search::Reply::Note(context) => Verdict::Inject { context },
        _ => Verdict::Allow,
    }
}

/// A busca por nome de arquivo (`Glob`): as palavras do padrão de nome
/// (`**/*payment*.ts` dá `payment`) vão à mesma triagem do mapa da busca por
/// palavra, e a busca roda como veio, com a linha da marca ou do que o mapa
/// não achou ([`word_search::names_search`]). O padrão sem palavra
/// (`**/*.ts`), a pasta fora do projeto ou sem código do mapa, e a chave
/// `search.answer` desligada passam calados.
fn glob_verdict(root: &str, input: &HookInput, ctx: &Ctx) -> Verdict {
    let ti = &input.tool_input;
    let text = |field: &str| ti.get(field).and_then(serde_json::Value::as_str).filter(|value| !value.trim().is_empty());
    let Some(pattern) = text("pattern") else { return Verdict::Allow };
    let words = word_search::name_words(pattern, false);
    if words.is_empty() {
        return Verdict::Allow;
    }
    // A pasta do padrão: a `path` da ferramenta e as partes do padrão antes da
    // primeira que traz curinga (`packages/core/src/**/*.rs`).
    let parts: Vec<&str> = pattern.split('/').collect();
    let named = parts[..parts.len() - 1].iter().take_while(|part| !part.contains(['*', '?', '[', '{']));
    let base = input.cwd.as_deref().filter(|cwd| !cwd.is_empty()).unwrap_or(root);
    let mut folder = std::path::PathBuf::from(if pattern.starts_with('/') { "/" } else { text("path").unwrap_or(".") });
    folder.extend(named.filter(|part| !part.is_empty()));
    let Some(folder) = code_route::project_path(root, base, &folder.to_string_lossy()).filter(|folder| folder.abs.is_dir())
    else {
        return Verdict::Allow;
    };
    let filters = word_search::extension_filters(pattern);
    let search = word_search::names_search(&words, std::slice::from_ref(&folder), &filters);
    match search_reply(root, input, ctx, &search) {
        word_search::Reply::Note(context) => Verdict::Inject { context },
        _ => Verdict::Allow,
    }
}

/// A resposta do gancho à busca `search`, com a chamada medida da busca
/// parcial gravada na spec da conversa. É o único lugar em que os ganchos
/// ligam a busca por palavra, que é parte compartilhada, à gravação da
/// conversa, que é de quem a grava.
pub(crate) fn search_reply(root: &str, input: &HookInput, ctx: &Ctx, search: &word_search::Search<'_>) -> word_search::Reply {
    word_search::hook_reply(root, input, ctx, search, &record_measured_call)
}

/// A primeira resposta de `rules` para `target`; sem resposta, passa.
pub(crate) fn judge(rules: &[&dyn WriteRule], target: &WriteTarget, at: &WriteContext) -> Verdict {
    rules.iter().find_map(|rule| rule.judge(target, at)).unwrap_or(Verdict::Allow)
}

// O preenchimento do catálogo mora em `shared::say`; os leitores que já o
// pediam por aqui seguem pedindo.
pub(crate) use crate::shared::say::say;

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

/// A leitura do `mustard.json` que guarda a chave do Jev volta com o arquivo,
/// e a chave trocada por `***`: a chave nunca entra na conversa.
pub(crate) struct ConfigKeyRule;

impl WriteRule for ConfigKeyRule {
    fn judge(&self, _target: &WriteTarget, at: &WriteContext) -> Option<Verdict> {
        at.config_key.clone().map(|reason| Verdict::Deny { reason })
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

/// A leitura inteira de um arquivo de código do mapa que traria mais linhas
/// que o teto volta com as partes dele e o comando que traz só a parte certa
/// ([`code_route::whole_read`]). Quem vai editar lê o trecho com `offset` e
/// `limit`: a edição aceita o arquivo lido por um trecho.
pub(crate) struct WholeReadRule;

impl WriteRule for WholeReadRule {
    fn judge(&self, _target: &WriteTarget, at: &WriteContext) -> Option<Verdict> {
        at.whole_read.clone().map(|reason| Verdict::Deny { reason })
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

/// A conversa de teste da busca: a spec aberta e a sessão ligada a ela, e as
/// chamadas medidas que a busca por palavra gravou nela.
#[cfg(test)]
pub(crate) mod conversation_fixture {
    use std::path::Path;

    use mustard_core::domain::spec_state::SpecState;
    use serde_json::{json, Map, Value};

    use crate::shared::context::session::bind_session_spec;
    use crate::shared::spec_state::DiskSpecState;

    /// Abre a spec `spec` no projeto e liga a sessão `session` a ela: a spec
    /// da conversa, onde a chamada medida da busca é gravada.
    pub(crate) fn converse(root: &Path, spec: &str, session: &str) {
        std::fs::create_dir_all(root.join(".claude/spec").join(spec)).expect("spec folder");
        let opened = crate::commands::spec_events::write::record_open(root, spec, &format!("feature/{spec}"), "dev");
        assert_eq!(opened, Ok(true), "the spec opens");
        bind_session_spec(&root.to_string_lossy(), session, spec);
    }

    /// As chamadas `word search` gravadas na spec `spec`, com os campos de
    /// cada uma.
    pub(crate) fn word_searches(root: &Path, spec: &str) -> Vec<Map<String, Value>> {
        DiskSpecState::new(root)
            .log(spec)
            .map(|log| {
                log.visible()
                    .into_iter()
                    .filter(|event| event.event_type == "call" && event.fields.get("command") == Some(&json!("word search")))
                    .map(|event| event.fields.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::conversation_fixture::{converse, word_searches};
    use crate::shared::code_route::fixture;
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
            whole_read: None,
            config_key: None,
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
            whole_read: None,
            config_key: None,
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

    /// A resposta do despachante à ferramenta `tool` com `tool_input`, chamada
    /// de `cwd` na sessão `session`: o caminho que a sessão usa, pelo registro
    /// dos ganchos. Sem sessão, a resposta do mapa à busca não tem onde
    /// guardar a busca respondida e passa.
    fn hook_in(cwd: &Path, tool: &str, tool_input: Value, session: Option<&str>) -> Verdict {
        let input = HookInput {
            tool_name: Some(tool.to_string()),
            tool_input,
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(cwd.to_string_lossy().into_owned()),
            session_id: session.map(str::to_string),
            ..HookInput::default()
        };
        crate::dispatch::run_event(Some(Trigger::PreToolUse), &input).verdict
    }

    /// O mesmo, numa chamada sem sessão.
    fn hook_on(cwd: &Path, tool: &str, tool_input: Value) -> Verdict {
        hook_in(cwd, tool, tool_input, None)
    }

    /// O motivo de uma recusa; qualquer outra resposta derruba o teste.
    fn refused(verdict: Verdict, what: &str) -> String {
        match verdict {
            Verdict::Deny { reason } => reason,
            other => panic!("{what} is refused, got {other:?}"),
        }
    }

    /// A nota que vai junto da busca, que roda como veio; qualquer outra
    /// resposta derruba o teste.
    fn noted(verdict: Verdict, what: &str) -> String {
        match verdict {
            Verdict::Inject { context } => context,
            other => panic!("{what} runs with a note, got {other:?}"),
        }
    }

    /// A leitura inteira de um arquivo de código do mapa com mais de 300
    /// linhas é recusada, nos dois idiomas: o motivo traz o tamanho, o
    /// comando do trecho pronto para o arquivo e as partes com as linhas,
    /// sem os campos.
    #[test]
    fn the_whole_read_of_a_large_mapped_file_is_refused_with_its_parts() {
        for (tag, words) in [("pt-BR", "Partes:"), ("en-US", "Parts:")] {
            let (_dir, root) = fixture::project(&format!(r#"{{"language":{{"text":"{tag}"}}}}"#), true);
            let reason = refused(
                hook_on(&root, "Read", json!({ "file_path": abs(&root, "src/big.rs") })),
                "the whole read of a large mapped file",
            );
            assert!(reason.contains("`mustard-rt run map slice --file src/big.rs --name "), "{tag}: {reason}");
            assert!(reason.contains("400"), "{tag}: {reason}");
            assert!(reason.contains(&format!("{words} Alpha 1-150, alpha 151-400.")), "{tag}: {reason}");
            assert!(!reason.contains("size"), "a field is not a part: {reason}");
        }
    }

    /// A leitura com faixa de linhas, a de um arquivo pequeno e a de um
    /// arquivo grande que o mapa não guarda passam.
    #[test]
    fn a_read_with_a_range_a_small_file_or_a_file_off_the_map_passes() {
        let (_dir, root) = fixture::project("{}", true);
        let big = abs(&root, "src/big.rs");
        for tool_input in [
            json!({ "file_path": big, "offset": 120, "limit": 40 }),
            json!({ "file_path": big, "limit": 350 }),
            json!({ "file_path": abs(&root, "src/small.rs") }),
            json!({ "file_path": abs(&root, "docs/big.md") }),
        ] {
            assert_eq!(hook_on(&root, "Read", tool_input.clone()), Verdict::Allow, "{tool_input}");
        }
    }

    /// Um arquivo com os testes dentro cuja parte de produção cabe no limite
    /// continua cortado antes dos testes; quando a parte de produção passa do
    /// limite, a leitura é recusada, e as partes dizem onde os testes começam.
    #[test]
    fn the_production_part_decides_between_the_cut_and_the_refusal() {
        let (_dir, root) = fixture::project("{}", true);
        let short = abs(&root, "src/tested.rs");
        match hook_on(&root, "Read", json!({ "file_path": short })) {
            Verdict::Rewrite { tool_input, .. } => assert_eq!(tool_input, json!({ "file_path": short, "limit": 100 })),
            other => panic!("the short production part is cut, got {other:?}"),
        }
        let reason = refused(
            hook_on(&root, "Read", json!({ "file_path": abs(&root, "src/long_tested.rs") })),
            "the long production part",
        );
        assert!(reason.contains("350"), "{reason}");
        assert!(reason.contains("delta 1-350; testes a partir da linha 351."), "{reason}");
    }

    /// Sem mapa, ou com um mapa que não se lê, a leitura inteira e a busca
    /// passam: o erro da trava nunca segura a ação.
    #[test]
    fn without_a_readable_map_the_read_and_the_search_pass() {
        let (_none, bare) = fixture::project("{}", false);
        let (_broken, garbled) = fixture::project("{}", false);
        mustard_core::io::project_map::write_text(&garbled, "isto não é um mapa").expect("garbled map");
        for root in [&bare, &garbled] {
            assert_eq!(hook_on(root, "Read", json!({ "file_path": abs(root, "src/big.rs") })), Verdict::Allow);
            assert_eq!(hook_on(root, "Grep", json!({ "pattern": "Alpha", "path": abs(root, "src") })), Verdict::Allow);
        }
        let (_none, without) = word_search::fixture::repo("{}");
        let (_broken, unreadable) = word_search::fixture::repo("{}");
        std::fs::remove_file(mustard_core::io::project_map::model_path(&without)).expect("no map");
        mustard_core::io::project_map::write_text(&unreadable, "isto não é um mapa").expect("garbled map");
        for (n, root) in [&without, &unreadable].into_iter().enumerate() {
            let tool_input = json!({ "pattern": "calcular_frete", "path": abs(root, "src") });
            assert_eq!(hook_in(root, "Grep", tool_input, Some(&format!("sem-mapa-{n}"))), Verdict::Allow, "{root:?}");
        }
    }

    /// Numa cópia de trabalho do projeto, fora da pasta dele, a leitura
    /// inteira de um arquivo grande do mapa é recusada como no projeto. Com
    /// linhas novas no topo do arquivo da cópia, as partes saem com as linhas
    /// da cópia: dez linhas a mais levam `Alpha` para 11-160 e `alpha` para
    /// 161-410; cinco a mais levam `delta` para 6-355, e os testes para a
    /// linha 356.
    #[test]
    fn the_whole_read_inside_a_working_copy_is_refused_with_the_copy_lines() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tmp_root = std::fs::canonicalize(tmp.path()).expect("tempdir resolvida");
        let tmp_root = std::path::PathBuf::from(tmp_root.to_string_lossy().trim_start_matches(r"\\?\").to_string());
        let main = tmp_root.join("repo");
        std::fs::create_dir_all(&main).expect("main");
        repo_on(&main, "dev");
        git(&main, &["worktree", "add", "-q", "../copy", "-b", "work"]);
        std::fs::write(main.join("mustard.json"), "{}").expect("config");
        mustard_core::io::project_map::write_text(&main, fixture::MAP).expect("map");
        let copy = tmp_root.join("copy");
        fixture::write_files(&main);
        fixture::write_files(&copy);

        let reason = refused(
            hook_on(&copy, "Read", json!({ "file_path": abs(&copy, "src/big.rs") })),
            "the whole read inside the working copy",
        );
        assert!(reason.contains("--file src/big.rs"), "{reason}");
        assert!(reason.contains("Alpha 1-150, alpha 151-400."), "{reason}");
        let ranged = json!({ "file_path": abs(&copy, "src/big.rs"), "offset": 1, "limit": 10 });
        assert_eq!(hook_on(&copy, "Read", ranged), Verdict::Allow);

        let on_top = |count: usize, file: &str| {
            let text = std::fs::read_to_string(main.join(file)).expect("project file");
            std::fs::write(copy.join(file), format!("{}{text}", "// nova\n".repeat(count))).expect("copy file");
        };
        on_top(10, "src/big.rs");
        on_top(5, "src/long_tested.rs");
        let moved = refused(
            hook_on(&copy, "Read", json!({ "file_path": abs(&copy, "src/big.rs") })),
            "the whole read of the moved file",
        );
        assert!(moved.contains("410"), "{moved}");
        assert!(moved.contains("Alpha 11-160, alpha 161-410."), "{moved}");
        let tested = refused(
            hook_on(&copy, "Read", json!({ "file_path": abs(&copy, "src/long_tested.rs") })),
            "the whole read of the moved tested file",
        );
        assert!(tested.contains("355"), "{tested}");
        assert!(tested.contains("delta 6-355; testes a partir da linha 356."), "{tested}");
    }

    /// A leitura e a busca do `mustard.json` que guarda a chave são
    /// recusadas; o motivo traz o arquivo com a chave trocada, e nunca a
    /// chave. Sem a chave, a leitura passa.
    #[test]
    fn the_config_file_with_the_key_is_shown_without_it() {
        let config = format!(r#"{{"language": {{"text": "pt-BR"}}, "jev": {{"key": "{}"}}}}"#, fixture::FAKE_KEY);
        let (_dir, root) = fixture::project(&config, true);
        let file = abs(&root, "mustard.json");
        for (tool, tool_input) in [
            ("Read", json!({ "file_path": file })),
            ("Grep", json!({ "pattern": "jev", "path": file })),
            ("Grep", json!({ "pattern": "key", "path": "mustard.json" })),
        ] {
            let reason = refused(hook_on(&root, tool, tool_input.clone()), "the config file with the key");
            assert!(!reason.contains(fixture::FAKE_KEY), "{tool} {tool_input}: the key leaked");
            assert!(reason.contains(r#""key": "***""#), "{tool} {tool_input}: {reason}");
            assert!(reason.contains(r#""language": {"text": "pt-BR"}"#), "the rest of the file stays: {reason}");
        }

        let (_plain, plain) = fixture::project(r#"{"language": {"text": "pt-BR"}}"#, true);
        assert_eq!(hook_on(&plain, "Read", json!({ "file_path": abs(&plain, "mustard.json") })), Verdict::Allow);
    }

    /// A busca por nome de arquivo (`Glob`) com uma palavra do nome que o mapa
    /// conhece roda como veio, com uma linha só da marca; a pasta do padrão e
    /// a `path` da ferramenta valem; o nome que o mapa não acha traz a linha
    /// do que ele não achou. O padrão sem palavra, o de documento, o de pasta
    /// fora do código, o sem sessão e o com a chave `search.answer` desligada
    /// passam calados.
    #[test]
    fn a_glob_with_a_word_of_the_name_runs_with_one_line_of_the_mark() {
        let (_dir, root) = word_search::fixture::repo("{}");
        for (n, tool_input) in [
            json!({ "pattern": "**/*frete*.rs" }),
            json!({ "pattern": "src/**/*frete*.rs" }),
            json!({ "pattern": "**/*frete*.rs", "path": "src" }),
            json!({ "pattern": "*frete*", "path": abs(&root, "src") }),
            json!({ "pattern": format!("{}/**/*frete*.rs", abs(&root, "src")) }),
        ]
        .into_iter()
        .enumerate()
        {
            match hook_in(&root, "Glob", tool_input.clone(), Some(&format!("glob{n}"))) {
                Verdict::Inject { context } => {
                    assert_eq!(context.lines().count(), 1, "{tool_input}: {context}");
                    assert!(context.starts_with("Cravado."), "{tool_input}: {context}");
                }
                other => panic!("{tool_input}: the glob runs with a line, got {other:?}"),
            }
        }
        match hook_in(&root, "Glob", json!({ "pattern": "**/*zzyzx*.rs" }), Some("glob-nada")) {
            Verdict::Inject { context } => assert!(context.starts_with("Não achei"), "{context}"),
            other => panic!("the glob runs with the line of what the map lacks, got {other:?}"),
        }
        for tool_input in [
            json!({ "pattern": "**/*.rs" }),
            json!({ "pattern": "src/**/*.{rs,toml}" }),
            json!({ "pattern": "**/*frete*.md" }),
            json!({ "pattern": "docs/**/*frete*" }),
            json!({ "pattern": "**/*frete*.rs", "path": "fora-do-projeto" }),
        ] {
            assert_eq!(hook_in(&root, "Glob", tool_input.clone(), Some("glob-calado")), Verdict::Allow, "{tool_input}");
        }
        assert_eq!(hook_on(&root, "Glob", json!({ "pattern": "**/*frete*.rs" })), Verdict::Allow, "no session");
        let (_off, off) = word_search::fixture::repo(r#"{"search":{"answer":false}}"#);
        assert_eq!(hook_in(&off, "Glob", json!({ "pattern": "**/*frete*.rs" }), Some("glob-off")), Verdict::Allow);
    }

    /// A busca que mostra as linhas de um nome do mapa numa pasta de código —
    /// a pasta dada, a raiz sem pasta, com um filtro de código, com a opção
    /// de caixa e com o tipo de arquivo — é respondida no lugar dela, agrupada
    /// por função com a linha de começo e a de fim.
    #[test]
    fn a_search_for_a_mapped_name_in_a_code_folder_is_answered_by_function() {
        let (_dir, root) = word_search::fixture::repo("{}");
        for (n, tool_input) in [
            json!({ "pattern": "calcular_frete", "path": abs(&root, "src"), "output_mode": "content" }),
            json!({ "pattern": "calcular_frete", "path": "src", "output_mode": "content" }),
            json!({ "pattern": "calcular_frete", "output_mode": "content" }),
            json!({ "pattern": "calcular_frete", "glob": "*.rs", "output_mode": "content" }),
            json!({ "pattern": "calcular_frete", "output_mode": "content", "-n": true }),
            json!({ "pattern": "CALCULAR_FRETE", "-i": true, "output_mode": "content" }),
            json!({ "pattern": "calcular_frete", "type": "rust", "output_mode": "content" }),
        ]
        .into_iter()
        .enumerate()
        {
            let reason = refused(hook_in(&root, "Grep", tool_input.clone(), Some(&format!("g{n}"))), "the search for a mapped name");
            assert!(reason.starts_with("Cravado."), "{tool_input}: {reason}");
            assert!(reason.contains("src/frete.rs\n  2-6 calcular_frete (2)"), "{tool_input}: {reason}");
            assert!(reason.contains("src/pedido.rs\n  1-4 fechar_pedido (2)"), "{tool_input}: {reason}");
        }
    }

    /// A busca que só lista nomes de arquivo (o modo de saída de sempre) ou só
    /// conta roda como veio, com uma linha só da marca: cravada ou parcial com
    /// a palavra que falta. Ela não vale como respondida, e a busca que mostra
    /// as linhas, logo depois, recebe a resposta por função.
    #[test]
    fn a_search_that_only_lists_names_or_counts_runs_plain_with_one_line_of_the_mark() {
        let (_dir, root) = word_search::fixture::repo("{}");
        for tool_input in [
            json!({ "pattern": "calcular_frete" }),
            json!({ "pattern": "calcular_frete", "output_mode": "files_with_matches", "path": "src" }),
            json!({ "pattern": "calcular_frete", "output_mode": "count" }),
        ] {
            match hook_in(&root, "Grep", tool_input.clone(), Some("nomes")) {
                Verdict::Inject { context } => {
                    assert_eq!(context.lines().count(), 1, "{tool_input}: {context}");
                    assert!(context.starts_with("Cravado."), "{tool_input}: {context}");
                    assert!(!context.contains("src/frete.rs"), "{tool_input}: no answer goes with it: {context}");
                }
                other => panic!("{tool_input}: the plain search runs with a line, got {other:?}"),
            }
        }
        let with_missing_word = json!({ "pattern": "calcular_frete|desconto_frete|imposto", "output_mode": "count" });
        match hook_in(&root, "Grep", with_missing_word, Some("nomes")) {
            Verdict::Inject { context } => {
                assert!(context.starts_with("Cravado.") && !context.contains("imposto"), "{context}");
            }
            other => panic!("the pinned search runs with a line, got {other:?}"),
        }
        let lines = json!({ "pattern": "calcular_frete", "output_mode": "content" });
        let reason = refused(hook_in(&root, "Grep", lines, Some("nomes")), "the search that shows lines");
        assert!(reason.contains("src/frete.rs\n  2-6 calcular_frete (2)"), "{reason}");
    }

    /// A busca com uma palavra que o primeiro arquivo do mapa não traz em
    /// campo forte continua cravada: a resposta cita só as palavras achadas,
    /// não pede nova busca e traz a linha do comentário com a palavra solta.
    #[test]
    fn a_search_with_a_word_the_map_lacks_is_answered_as_pinned_without_asking_again() {
        let (_dir, root) = word_search::fixture::repo("{}");
        let tool_input = json!({ "pattern": "calcular_frete|desconto_frete|imposto", "output_mode": "content" });
        let reason = refused(hook_in(&root, "Grep", tool_input, Some("parcial")), "the pinned search");
        assert!(reason.starts_with("Cravado."), "{reason}");
        assert!(!reason.contains("Falta") && !reason.contains("Busque de novo"), "{reason}");
        assert!(reason.contains("src/frete.rs\n  2-6 calcular_frete (2, 3)"), "{reason}");
    }

    /// Sem achado no mapa a busca comum roda, com uma linha do que o mapa não
    /// achou; a mesma busca repetida na sessão roda sem a linha, e a que a
    /// resposta já deu passa em qualquer modo de saída.
    #[test]
    fn a_search_the_map_finds_nothing_for_runs_plain_with_one_line() {
        let (_dir, root) = word_search::fixture::repo("{}");
        let tool_input = json!({ "pattern": "zzznada", "path": "src" });
        match hook_in(&root, "Grep", tool_input.clone(), Some("nada")) {
            Verdict::Inject { context } => {
                assert_eq!(context.lines().count(), 1, "{context}");
                assert!(context.contains(r#"grep -rniE "zzznada" ."#), "{context}");
            }
            other => panic!("the plain search runs with a line, got {other:?}"),
        }
        assert_eq!(hook_in(&root, "Grep", tool_input, Some("nada")), Verdict::Allow);

        let found = json!({ "pattern": "calcular_frete", "path": "src", "output_mode": "content" });
        refused(hook_in(&root, "Grep", found.clone(), Some("repete")), "the first search");
        for repeated in [found.clone(), json!({ "pattern": "calcular_frete", "path": "src", "output_mode": "count" })] {
            assert_eq!(hook_in(&root, "Grep", repeated.clone(), Some("repete")), Verdict::Allow, "{repeated}");
        }
        refused(hook_in(&root, "Grep", found, Some("outra")), "another session");
    }

    /// A chave `search.answer` desligada deixa passar a busca que seria
    /// respondida.
    #[test]
    fn the_answer_key_off_lets_the_search_pass() {
        let tool_input = json!({ "pattern": "calcular_frete", "path": "src", "output_mode": "content" });
        let (_off, off) = word_search::fixture::repo(r#"{"search":{"answer":false}}"#);
        assert_eq!(hook_in(&off, "Grep", tool_input.clone(), Some("off")), Verdict::Allow);
        let (_on, on) = word_search::fixture::repo(r#"{"search":{"answer":true}}"#);
        refused(hook_in(&on, "Grep", tool_input, Some("on")), "the key on");
    }

    /// A busca de um nome cujo `glob` tira todo o código do mapa da pasta —
    /// um filtro de saída, vários separados por espaço ou vírgula, ou o de
    /// saída que um de entrada não desfaz depois — passa; o `glob` de
    /// entrada que vem depois do de saída traz o código de volta, e o de
    /// saída de outro tipo de arquivo não tira o código: a busca é
    /// respondida. Vários filtros no `glob` valem todos, não só o último.
    #[test]
    fn a_search_whose_glob_leaves_out_all_the_mapped_code_passes() {
        let (_dir, root) = word_search::fixture::repo("{}");
        for (n, tool_input) in [
            json!({ "pattern": "calcular_frete", "glob": "!*.rs" }),
            json!({ "pattern": "calcular_frete", "glob": "!*.{rs,toml}", "path": abs(&root, "src") }),
            json!({ "pattern": "calcular_frete", "glob": "!*.rs !*.md" }),
            json!({ "pattern": "calcular_frete", "glob": "!*.rs,!*.md" }),
            json!({ "pattern": "calcular_frete", "glob": "*.rs !*.rs" }),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(hook_in(&root, "Grep", tool_input.clone(), Some(&format!("a{n}"))), Verdict::Allow, "{tool_input}");
        }
        for (n, tool_input) in [
            json!({ "pattern": "calcular_frete", "glob": "!*.md", "output_mode": "content" }),
            json!({ "pattern": "calcular_frete", "glob": "!*.rs *.rs", "output_mode": "content" }),
            json!({ "pattern": "calcular_frete", "glob": "*.rs *.md", "output_mode": "content" }),
            json!({ "pattern": "calcular_frete", "glob": "!frete.rs", "output_mode": "content" }),
        ]
        .into_iter()
        .enumerate()
        {
            let reason = refused(hook_in(&root, "Grep", tool_input.clone(), Some(&format!("b{n}"))), "the search for a mapped name");
            assert!(reason.contains("calcular_frete"), "{tool_input}: {reason}");
        }
    }

    /// A busca num arquivo só, com o tipo ou o `glob` de outra família, só em
    /// documentos, numa pasta sem código do mapa, fora do projeto, sem
    /// sessão ou em várias linhas passa.
    #[test]
    fn a_search_in_one_file_or_outside_the_code_passes() {
        let (_dir, root) = word_search::fixture::repo("{}");
        let outside = tempfile::tempdir().expect("tempdir");
        for (n, tool_input) in [
            json!({ "pattern": "calcular_frete", "path": abs(&root, "src/frete.rs") }),
            json!({ "pattern": "calcular_frete", "glob": "*.md" }),
            json!({ "pattern": "calcular_frete", "path": abs(&root, "docs") }),
            json!({ "pattern": "calcular_frete", "path": outside.path().to_string_lossy() }),
            json!({ "pattern": "calcular_frete", "type": "rust", "glob": "*.rs" }),
            json!({ "pattern": "calcular_frete", "type": "fortran" }),
            json!({ "pattern": "calcular_frete.*peso", "multiline": true }),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(hook_in(&root, "Grep", tool_input.clone(), Some(&format!("c{n}"))), Verdict::Allow, "{tool_input}");
        }
        let unnamed = json!({ "pattern": "calcular_frete", "path": "src" });
        assert_eq!(hook_on(&root, "Grep", unnamed), Verdict::Allow, "no session, no answer");
    }

    /// Numa cópia de trabalho do projeto, a busca lê a árvore da cópia: o
    /// arquivo que a onda mudou vem relido, marcado como mudado e com as
    /// linhas da cópia.
    #[test]
    fn a_search_inside_a_working_copy_rereads_the_files_the_wave_changed() {
        let (dir, root) = word_search::fixture::repo("{}");
        let copy = dir.path().parent().expect("parent").join(format!("copia-grep-{}", std::process::id()));
        word_search::fixture::git(&root, &["worktree", "add", "-q", &copy.to_string_lossy(), "-b", "onda"]);
        let copy = std::fs::canonicalize(&copy).expect("copy");
        std::fs::write(copy.join("src/frete.rs"), format!("// a\n// b\n// c\n{}", word_search::fixture::FRETE)).expect("edit");
        let tool_input = json!({ "pattern": "calcular_frete", "path": abs(&copy, "src"), "output_mode": "content" });
        let reason = refused(hook_in(&copy, "Grep", tool_input, Some("copia")), "a search in the working copy");
        assert!(reason.contains("src/frete.rs (mudado nesta onda)\n  5-9 calcular_frete (5)"), "{reason}");
        assert!(reason.contains("src/pedido.rs\n  1-4 fechar_pedido (2)"), "{reason}");
        word_search::fixture::git(&root, &["worktree", "remove", "--force", &copy.to_string_lossy()]);
    }

    /// A resposta do mapa nunca traz o `mustard.json` com a chave; a busca
    /// comum que roda por falta de achado segue sob a trava da chave.
    #[test]
    fn the_answer_never_carries_the_key_and_the_plain_search_still_hides_it() {
        let config = format!(r#"{{"jev": {{"key": "{}"}}}}"#, fixture::FAKE_KEY);
        let (_dir, root) = word_search::fixture::repo(&config);
        let answered = json!({ "pattern": "calcular_frete", "output_mode": "content" });
        let reason = refused(hook_in(&root, "Grep", answered, Some("chave")), "the answer");
        assert!(reason.starts_with("Cravado.") && !reason.contains(fixture::FAKE_KEY), "{reason}");
        let plain = json!({ "pattern": "zzznada", "glob": "*.json", "output_mode": "content" });
        let swept = refused(hook_in(&root, "Grep", plain, Some("chave")), "the plain search through the key file");
        assert!(swept.contains("mustard.json") && !swept.contains(fixture::FAKE_KEY), "{swept}");
    }

    /// A busca que a rota do nome deixa passar por causa do `glob` — só
    /// documentos e configuração, com o código de fora — continua sob a trava
    /// da chave: traz o `mustard.json` com a chave e é recusada sem mostrá-la.
    #[test]
    fn a_search_the_route_lets_pass_still_hides_the_key() {
        let config = format!(r#"{{"jev": {{"key": "{}"}}}}"#, fixture::FAKE_KEY);
        let (_dir, root) = fixture::project(&config, true);
        let tool_input = json!({ "pattern": "Alpha", "glob": "*.json !*.rs", "output_mode": "content" });
        let reason = refused(hook_on(&root, "Grep", tool_input.clone()), "the search through the key file");
        assert!(!reason.contains(fixture::FAKE_KEY), "{tool_input}: the key leaked");
        assert!(reason.contains("mustard.json"), "{tool_input}: {reason}");
        let names_only = json!({ "pattern": "Alpha", "glob": "!*.rs" });
        assert_eq!(hook_on(&root, "Grep", names_only), Verdict::Allow);
    }

    /// A busca que traz as linhas numa pasta com o `mustard.json` que guarda
    /// a chave, com um `glob` que casa com o nome dele, é recusada sem
    /// mostrar a chave. Sem o `glob`, com um `glob` que não casa, no modo que
    /// só lista arquivos ou conta, numa pasta de dentro ou sem a chave, passa.
    #[test]
    fn a_search_whose_glob_reaches_the_key_file_is_refused() {
        let config = format!(r#"{{"jev": {{"key": "{}"}}}}"#, fixture::FAKE_KEY);
        let (_dir, root) = fixture::project(&config, true);
        for tool_input in [
            json!({ "pattern": "key", "glob": "*.json", "output_mode": "content" }),
            json!({ "pattern": "key", "glob": "*.{json,md}", "output_mode": "content", "path": abs(&root, ".") }),
            json!({ "pattern": "key", "glob": "*.rs mustard.json", "output_mode": "content" }),
        ] {
            let reason = refused(hook_on(&root, "Grep", tool_input.clone()), "the search through the key file");
            assert!(!reason.contains(fixture::FAKE_KEY), "{tool_input}: the key leaked");
            assert!(reason.contains("mustard.json") && reason.contains("`type`"), "{tool_input}: {reason}");
        }
        for tool_input in [
            json!({ "pattern": "key", "output_mode": "content" }),
            json!({ "pattern": "key", "glob": "*.rs", "output_mode": "content" }),
            json!({ "pattern": "key", "glob": "*.json" }),
            json!({ "pattern": "key", "glob": "*.json", "output_mode": "count" }),
            json!({ "pattern": "key", "glob": "*.json", "output_mode": "content", "path": abs(&root, "src") }),
        ] {
            assert_eq!(hook_on(&root, "Grep", tool_input.clone()), Verdict::Allow, "{tool_input}");
        }
        let (_plain, plain) = fixture::project("{}", true);
        let tool_input = json!({ "pattern": "key", "glob": "*.json", "output_mode": "content" });
        assert_eq!(hook_on(&plain, "Grep", tool_input), Verdict::Allow);
    }

    /// A busca parcial do `Grep`, pelo gancho de verdade, vai ao filtro que a
    /// sessão tem, e a chamada medida dela fica gravada na spec da conversa:
    /// o comando, o filtro, quantos candidatos foram e quantas peças
    /// voltaram. A nota que vai junto da busca traz só a peça que o filtro
    /// entregou.
    #[test]
    fn a_partial_grep_through_the_hook_records_its_measured_call_in_the_conversation_spec() {
        let (_dir, root) = word_search::fixture::repo("{}");
        converse(&root, "conversa", "s-grava");
        let judge = word_search::fixture::Judge::sure_of(&[("calcular_frete", 0.9)]);
        let tool_input = json!({ "pattern": "imposto", "output_mode": "content" });

        let verdict = judge.installed(|| hook_in(&root, "Grep", tool_input, Some("s-grava")));

        let reason = noted(verdict, "the partial search that shows lines");
        assert!(reason.contains("src/frete.rs\n  2-6 calcular_frete (3)"), "{reason}");
        assert!(!reason.contains("desconto_frete"), "only what the filter delivered: {reason}");
        assert_eq!(judge.calls(), 1, "the filter of the session is the one asked");
        let calls = word_searches(&root, "conversa");
        assert_eq!(calls.len(), 1, "{calls:?}");
        let call = &calls[0];
        assert_eq!(call["author"], json!("binary"));
        assert_eq!(call["result"], json!("ok"));
        assert_eq!(
            [&call["filter"], &call["candidates"], &call["returned"]],
            [&json!("jev"), &json!(2), &json!(1)],
            "the measure of the filter travels with the call: {call:?}"
        );
    }

    /// O filtro que falha na busca parcial deixa a resposta da triagem, e a
    /// chamada fica gravada mesmo assim, com o motivo no nome do filtro.
    #[test]
    fn a_partial_grep_whose_filter_fails_records_the_call_with_the_reason() {
        let (_dir, root) = word_search::fixture::repo("{}");
        converse(&root, "conversa", "s-falha");
        let judge = word_search::fixture::Judge::failing(mustard_core::domain::map_filter::FilterError::Timeout);
        let tool_input = json!({ "pattern": "imposto", "output_mode": "content" });

        let verdict = judge.installed(|| hook_in(&root, "Grep", tool_input, Some("s-falha")));

        let reason = noted(verdict, "the partial search with a failing filter");
        assert!(reason.contains("src/frete.rs"), "the triage answers: {reason}");
        let calls = word_searches(&root, "conversa");
        assert_eq!(calls.len(), 1, "{calls:?}");
        assert!(calls[0]["filter"].as_str().is_some_and(|name| name.starts_with("jev:")), "{:?}", calls[0]);
    }

    /// Nenhuma chamada é gravada quando a busca não chega ao filtro — a
    /// cravada, a que só lista nomes e a busca por nome de arquivo — nem
    /// quando a sessão não tem spec para receber a conversa; a resposta sai
    /// do mesmo jeito.
    #[test]
    fn a_search_the_filter_never_judges_or_a_session_with_no_spec_records_no_call() {
        let (_dir, root) = word_search::fixture::repo("{}");
        converse(&root, "conversa", "s-cala");
        let judge = word_search::fixture::Judge::sure_of(&[("calcular_frete", 0.9)]);

        judge.installed(|| {
            let pinned = json!({ "pattern": "fechar_pedido", "output_mode": "content" });
            refused(hook_in(&root, "Grep", pinned, Some("s-cala")), "the pinned search");
            let names_only = json!({ "pattern": "imposto", "output_mode": "files_with_matches" });
            assert!(matches!(hook_in(&root, "Grep", names_only, Some("s-cala")), Verdict::Inject { .. }));
            let by_name = json!({ "pattern": "**/*frete*.rs" });
            assert!(matches!(hook_in(&root, "Glob", by_name, Some("s-cala")), Verdict::Inject { .. }));
        });
        assert_eq!(judge.calls(), 0, "the filter is never asked");
        assert!(word_searches(&root, "conversa").is_empty());

        let lines = json!({ "pattern": "imposto", "output_mode": "content" });
        let reason = noted(judge.installed(|| hook_in(&root, "Grep", lines, Some("s-sem-spec"))), "the search with no spec");
        assert!(reason.contains("src/frete.rs\n  2-6 calcular_frete (3)"), "the answer does not need the spec: {reason}");
        assert_eq!(judge.calls(), 1);
        assert!(word_searches(&root, "conversa").is_empty(), "a session bound to no spec writes to no spec");
    }

    /// A resposta do despachante ao `Grep`, como o gancho a escreve para o
    /// Claude Code, no evento de antes da ferramenta.
    fn written(cwd: &Path, tool_input: Value, session: &str) -> Value {
        let input = HookInput {
            tool_name: Some("Grep".to_string()),
            tool_input,
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(cwd.to_string_lossy().into_owned()),
            session_id: Some(session.to_string()),
            ..HookInput::default()
        };
        let outcome = crate::dispatch::run_event(Some(Trigger::PreToolUse), &input);
        let json = crate::hook_output::hook_specific_output("PreToolUse", &outcome).expect("the search is answered");
        serde_json::from_str(&json).expect("valid JSON")
    }

    /// A busca parcial pelo `Grep` roda com a nota do mapa junto, escrita só
    /// como contexto, sem decisão de permissão; a nota diz que é o que o mapa
    /// achou e não manda buscar de novo. A cravada segue no lugar da busca,
    /// com a recusa escrita como `deny`.
    #[test]
    fn a_partial_grep_runs_with_a_note_and_no_permission_while_the_pinned_one_is_denied() {
        let (_dir, root) = word_search::fixture::repo("{}");
        let partial = written(&root, json!({ "pattern": "imposto", "output_mode": "content" }), "s-parcial");
        let output = &partial["hookSpecificOutput"];
        assert!(output.get("permissionDecision").is_none(), "a note approves nothing: {partial}");
        let note = output["additionalContext"].as_str().expect("the note");
        assert!(note.starts_with("Parcial."), "{note}");
        assert!(note.contains("ao lado do resultado da busca"), "{note}");
        assert!(!note.contains("Busque de novo"), "{note}");
        assert!(note.contains("src/frete.rs\n  2-6 calcular_frete (3)"), "{note}");

        let pinned = written(&root, json!({ "pattern": "calcular_frete", "output_mode": "content" }), "s-cravada");
        assert_eq!(pinned["hookSpecificOutput"]["permissionDecision"], json!("deny"), "{pinned}");
        assert!(pinned["hookSpecificOutput"]["permissionDecisionReason"].as_str().is_some_and(|reason| reason.starts_with("Cravado.")), "{pinned}");
    }
}
