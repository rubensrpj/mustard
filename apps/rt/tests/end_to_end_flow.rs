// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! O fluxo inteiro de uma spec de teste, pelo binário de verdade, numa pasta
//! temporária: abrir, levantar, conferir o plano, aprovar pelo clique, duas
//! rodadas, fechar e abrir o pull request.
//!
//! A fala do usuário e o clique chegam pelos ganchos, como chegam numa sessão;
//! o agente da onda e o revisor são as linhas do fim que os textos deles
//! ensinam, e o provedor do pull request é um `gh` falso no começo do `PATH`.
//! No fim, cada passo do fluxo gravou uma chamada só — a aprovação nenhuma — e
//! a pasta da spec tem os três arquivos dela.
//!
//! Com o pull request aberto, a reabertura pedida pelo usuário o deixa fora de
//! rascunho, e o merge pelo Mustard espera a spec fechar de novo. O `pr-open`
//! seguinte refaz o corpo que segue igual ao gerado e deixa o que alguém
//! editou no provedor, e a rodada depois da reabertura também o deixa.
//!
//! O `pr-open` e a retomada da spec com o pull request aberto leem a esteira
//! do provedor e dizem o que ela respondeu; e o corpo do pull request que
//! alguém editou fica como está, enquanto o que segue igual ao gerado é
//! refeito.
//!
//! A mesma spec, num projeto com um submódulo e servidores locais, prova o
//! fluxo de submódulo: a branch de mesmo nome nos dois repositórios, o pull
//! request do submódulo aberto antes e o do principal como rascunho até o do
//! submódulo entrar e o ponteiro ser atualizado — pelo `pr-merge` e pela
//! conferência do início da sessão.
//!
//! O pronto do principal é medido pelo ponteiro que ele tinha na hora, e não
//! pela ordem das chamadas: os casos em que o ponteiro não acontece — commit
//! do submódulo que o merge não levou, envio que o servidor recusa — deixam o
//! principal como rascunho, e duas sessões conferindo ao mesmo tempo movem o
//! ponteiro uma vez só.
//!
//! Com uma cópia do programa ao lado de um scan de mentira, o mesmo projeto
//! prova que a abertura, a rodada e o fechamento, ao criar ou reler o mapa do
//! projeto, começam a leitura da história dos arquivos dele — e só a dele: a
//! cópia do mapa que a conferência depois da onda joga fora fica sem leitura.

#![cfg(unix)]

#[path = "support/binary.rs"]
mod binary;

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use mustard_core::domain::normalize::Languages;
use mustard_core::domain::spec_events::{SpecEvent, SpecLog};
use mustard_core::domain::spec_state::State;
use mustard_core::io::project_map as map_store;
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Value, json};

#[path = "support/executable.rs"]
mod executable;

#[path = "support/mod.rs"]
mod support;
#[path = "support/review.rs"]
mod wave_review;

const SPEC: &str = "ponta";
const GOAL: &str = "Trocar a saudação do programa.";
const SESSION: &str = "s-ponta";
/// A branch da spec, a mesma no principal e no submódulo.
const BRANCH: &str = "feature/ponta";
/// O submódulo do projeto de teste e o arquivo dele que a onda muda.
const SUB: &str = "libs/sub";
const SUB_FILE: &str = "libs/sub/lib.txt";

fn git(root: &Path, args: &[&str]) {
    git_out(root, args);
}

/// A saída do git, que precisa responder.
fn git_out(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(args).current_dir(root).output().expect("git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// O repositório `repo` com o nome e o e-mail de quem comita.
fn identify(repo: &Path) {
    git(repo, &["config", "user.email", "t@example.com"]);
    git(repo, &["config", "user.name", "t"]);
    git(repo, &["config", "commit.gpgsign", "false"]);
}

/// O `gh` falso do projeto com submódulo: anota cada chamada com o
/// repositório de onde veio (`sub` ou `main`), abre o pull request 3 no
/// submódulo e o 7 no principal, lembra quais estão abertos, em rascunho e
/// mergeados, e faz o merge do submódulo de verdade no servidor dele, por um
/// commit de merge. Ao marcar um como pronto, anota o ponteiro do submódulo
/// que o principal tinha naquele instante ([`pointer_when_ready`]): é assim
/// que o teste vê se o ponteiro veio antes do pronto.
const SUBMODULE_GH: &str = r#"#!/bin/sh
case "$(pwd -P)" in
  */libs/sub) repo=sub; number=3 ;;
  *) repo=main; number=7 ;;
esac
echo "$repo $*" >> "$GH_LOG"
mark="$GH_STATE/$repo"
case "$1 $2" in
"pr create")
  touch "$mark.open"
  case " $* " in *" --draft "*) touch "$mark.draft" ;; esac
  echo "https://github.com/exemplo/$repo/pull/$number"
  exit 0 ;;
"pr view")
  [ -f "$mark.open" ] || { echo 'no pull requests found' >&2; exit 1; }
  case "$*" in *statusCheckRollup*) echo '{"statusCheckRollup":[]}'; exit 0 ;; esac
  state=OPEN; [ -f "$mark.merged" ] && state=MERGED
  draft=false; [ -f "$mark.draft" ] && draft=true
  printf '{"number":%s,"title":"t","state":"%s","headRefName":"feature/ponta","baseRefName":"b","isDraft":%s,"url":"https://github.com/exemplo/%s/pull/%s"}
' "$number" "$state" "$draft" "$repo" "$number"
  exit 0 ;;
"pr merge")
  work="$GH_STATE/merge-$repo"
  git clone -q "$(git config --get remote.origin.url)" "$work"     && git -C "$work" -c user.email=t@example.com -c user.name=t merge -q --no-ff origin/feature/ponta -m "Merge pull request #$number"     && git -C "$work" push -q origin HEAD:main     && touch "$mark.merged"
  exit $? ;;
"pr ready")
  git rev-parse "HEAD:libs/sub" > "$GH_STATE/ready-$repo" 2>/dev/null
  rm -f "$mark.draft"; exit 0 ;;
"api --method") exit 0 ;;
esac
exit 1
"#;

/// O projeto de teste: um repositório com `main` e `dev`, parado em `dev`, com
/// as bases declaradas, o provedor do GitHub, o lint do projeto e o Mustard
/// fora do git; um servidor local vazio como `origin`; uma pasta pessoal
/// falsa; e o `gh` falso, que anota cada chamada e responde que a branch não
/// tem pull request e que o criado é o 7.
struct Project {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    bin: PathBuf,
    /// Os servidores locais do projeto: o do principal (`projeto.git`) e, com
    /// submódulo, o dele.
    remotes: PathBuf,
    /// A cópia do programa que roda ao lado do scan de mentira, quando o
    /// projeto tem um ([`Project::with_scan`]).
    rt: Option<PathBuf>,
}

impl Project {
    fn new() -> Self {
        Self::build(tempfile::tempdir().expect("tempdir"), false)
    }

    /// O projeto com o submódulo `libs/sub`, que vem de um servidor local com
    /// a base `main`; o principal tem o servidor dele, com `main` e `dev`; e o
    /// `gh` falso responde por repositório ([`SUBMODULE_GH`]).
    fn with_submodule() -> Self {
        Self::build(tempfile::tempdir().expect("tempdir"), true)
    }

    /// O projeto em `dir`, pela forma canônica dela: o que o programa grava
    /// e o que o teste espera citam o mesmo caminho. Onde a pasta temporária
    /// tem dois nomes — `/var` e `/private/var` no Mac —, o programa lê o
    /// projeto pelo nome resolvido, e o caminho montado com o outro nome
    /// nunca casaria com o que ele anota.
    fn build(dir: tempfile::TempDir, submodule: bool) -> Self {
        let base = dir.path().canonicalize().expect("the canonical temp folder");
        let root = base.join("projeto");
        let home = base.join("casa");
        let bin = base.join("bin");
        let remotes = base.join("servidores");
        for folder in [&root, &home, &bin, &remotes] {
            std::fs::create_dir_all(folder).expect("folder");
        }
        git(&root, &["init", "-q"]);
        identify(&root);
        git(&root, &["checkout", "-q", "-b", "main"]);
        std::fs::write(root.join(".git/info/exclude"), ".claude/\nmustard.json\ntarget/\n").expect("exclude");
        let config = json!({
            "language": {"text": "pt-BR"},
            "git": {"flow": {"*": "dev", "dev": "main"}, "provider": "github"},
            "lintCommand": "git --version",
        });
        std::fs::write(root.join("mustard.json"), config.to_string()).expect("config");
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::write(root.join("src/main.rs"), "fn main() {\n    println!(\"oi\");\n}\n").expect("code");
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "init"]);
        if submodule {
            let sub_server = remotes.join("sub.git");
            let seed = remotes.join("semente");
            git(&remotes, &["init", "-q", "--bare", "-b", "main", "sub.git"]);
            git(&remotes, &["init", "-q", "-b", "main", "semente"]);
            identify(&seed);
            std::fs::write(seed.join("lib.txt"), "a biblioteca\n").expect("the submodule file");
            git(&seed, &["add", "-A"]);
            git(&seed, &["commit", "-q", "-m", "biblioteca"]);
            git(&seed, &["push", "-q", &sub_server.to_string_lossy(), "main"]);
            let sub_url = sub_server.to_string_lossy().to_string();
            git(&root, &["-c", "protocol.file.allow=always", "submodule", "add", "-q", &sub_url, SUB]);
            identify(&root.join(SUB));
            git(&root, &["commit", "-q", "-m", "submodulo"]);
        }
        // O servidor do principal, sem a branch da spec: o `pr-open` a envia.
        git(&remotes, &["init", "-q", "--bare", "projeto.git"]);
        git(&root, &["remote", "add", "origin", &remotes.join("projeto.git").to_string_lossy()]);
        if submodule {
            git(&root, &["push", "-q", "origin", "main"]);
        }
        git(&root, &["checkout", "-q", "-b", "dev"]);
        if submodule {
            git(&root, &["push", "-q", "origin", "dev"]);
        }

        let gh = bin.join("gh");
        let script = if submodule {
            SUBMODULE_GH.to_string()
        } else {
            "#!/bin/sh\necho \"$*\" >> \"$GH_LOG\"\ncase \"$1 $2\" in\n\
             \"pr view\") echo 'no pull requests found' >&2; exit 1 ;;\n\
             \"pr create\") echo 'https://github.com/exemplo/projeto/pull/7'; exit 0 ;;\n\
             esac\nexit 1\n"
                .to_string()
        };
        executable::write_executable(&gh, &script);
        support::copies_leave_with_the_test(&root);
        Self { _dir: dir, root, home, bin, remotes, rt: None }
    }

    /// O projeto de [`Project::new`] com uma cópia do programa ao lado de um
    /// scan de mentira: cada passada grava no `--out` o mesmo mapa pronto e é
    /// anotada em `scan.log`, e cada leitura da história é anotada em
    /// `history.log`. A cópia é feita por outro processo (ver
    /// `support/executable.rs`), e o scan é o que está ao lado de quem roda.
    fn with_scan() -> Self {
        Self::with_scan_in(tempfile::tempdir().expect("tempdir"))
    }

    /// [`Project::with_scan`] na pasta temporária `dir`.
    fn with_scan_in(dir: tempfile::TempDir) -> Self {
        let mut project = Self::build(dir, false);
        let rt = project.bin.join("mustard-rt");
        let copied = Command::new("cp").arg(env!("CARGO_BIN_EXE_mustard-rt")).arg(&rt).status().expect("cp runs");
        assert!(copied.success(), "the copy of the program is made");

        let now = map_store::listing(&project.root).expect("inside git");
        let map = json!({
            "state": {"head": now.head, "listing": now.digest(), "base": now.base.name, "base_tip": now.base.tip},
            "modules": [{"path": "src/main.rs", "loc": 3, "declarations": [
                {"kind": "function", "name": "main", "line": 1, "end_line": 3}]}]
        });
        let template = project.home.join("map.db");
        map_store::save_at(&template, &map, "0.2.4+map-test", &Languages::new(["pt-BR", "en-US"])).expect("the map");

        let script = format!(
            "#!/bin/sh\ncase \"$1\" in\n\
             history-all) echo \"$@\" >> '{history}'; exit 0 ;;\n\
             scan) echo \"$@\" >> '{scans}'\n\
             mkdir -p \"$(dirname \"$4\")\" && cp '{template}' \"$4\"\n\
             echo '{{\"ok\":true,\"full\":true,\"read\":[],\"files\":1,\"head\":\"\"}}'; exit 0 ;;\n\
             *) exit 1 ;;\nesac\n",
            history = project.history_log().display(),
            scans = project.scan_log().display(),
            template = template.display(),
        );
        executable::write_executable(&project.bin.join("scan"), &script);
        project.rt = Some(rt);
        project
    }

    /// O projeto de [`Project::new`] como o código-fonte do Mustard: o pacote
    /// `mustard-rt` declarado em `apps/rt` e um `cargo` de mentira à frente do
    /// `PATH`, que anota a pasta e os argumentos de cada chamada em
    /// `cargo.log` e, com o arquivo `cargo-falha` na pasta pessoal, escreve um
    /// erro e sai vermelho.
    fn as_mustard_source() -> Self {
        let project = Self::new();
        std::fs::create_dir_all(project.root.join("apps/rt/src")).expect("apps/rt/src");
        std::fs::write(project.root.join("apps/rt/Cargo.toml"), "[package]\nname = \"mustard-rt\"\nversion = \"0.1.0\"\n")
            .expect("the manifest");
        std::fs::write(project.root.join("apps/rt/src/main.rs"), "fn main() {\n    println!(\"oi\");\n}\n").expect("the program");
        git(&project.root, &["add", "-A"]);
        git(&project.root, &["commit", "-q", "-m", "o programa"]);
        let script = format!(
            "#!/bin/sh\necho \"$PWD $*\" >> '{log}'\n\
             if [ -f '{fail}' ]; then echo 'error[E0425]: cannot find value `x`' >&2; exit 101; fi\nexit 0\n",
            log = project.cargo_log().display(),
            fail = project.home.join("cargo-falha").display(),
        );
        executable::write_executable(&project.bin.join("cargo"), &script);
        project
    }

    fn cargo_log(&self) -> PathBuf {
        self.home.join("cargo.log")
    }

    /// As chamadas ao `cargo` de mentira, na ordem, cada uma como
    /// `<pasta em que rodou> <argumentos>`.
    fn cargo_calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.cargo_log()).unwrap_or_default().lines().map(str::to_string).collect()
    }

    /// A pasta onde o programa compilado da versão em construção nasce, e a
    /// trava da compilação em segundo plano ao lado dela.
    fn build_folder(&self) -> PathBuf {
        let key = mustard_core::io::wave_prompt::development_build_dir(&self.root);
        self.home.join("build").join(key.file_name().expect("the project key"))
    }

    fn build_marker(&self) -> PathBuf {
        let folder = self.build_folder();
        let mut name = folder.file_name().expect("the folder name").to_os_string();
        name.push(".building");
        folder.with_file_name(name)
    }

    /// O commit atual do projeto, em 12 dígitos.
    fn head(&self) -> String {
        git_out(&self.root, &["rev-parse", "--short=12", "HEAD"])
    }

    /// Um programa compilado de mentira na pasta da versão em construção, que
    /// responde `says` ao `--version`.
    fn compiled_program_says(&self, says: &str) {
        let program = self.build_folder().join("mustard-dev").join("mustard-rt");
        std::fs::create_dir_all(program.parent().expect("development profile folder")).expect("the build folder");
        executable::write_executable(&program, &format!("#!/bin/sh\necho '{says}'\n"));
    }

    /// As chamadas ao `cargo` depois de esperar até `at_least` delas, por um
    /// tempo: a compilação do início da sessão roda em segundo plano.
    fn cargo_calls_after_waiting(&self, at_least: usize) -> Vec<String> {
        for _ in 0..200 {
            if self.cargo_calls().len() >= at_least {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        self.cargo_calls()
    }

    fn scan_log(&self) -> PathBuf {
        self.home.join("scan.log")
    }

    fn history_log(&self) -> PathBuf {
        self.home.join("history.log")
    }

    /// As passadas que o scan de mentira recebeu, na ordem, uma por linha.
    fn scan_passes(&self) -> Vec<String> {
        std::fs::read_to_string(self.scan_log()).unwrap_or_default().lines().map(str::to_string).collect()
    }

    /// As leituras da história que o scan de mentira recebeu, na ordem, sem
    /// esperar por nenhuma: o programa não espera o processo em segundo plano.
    fn history_reads(&self) -> Vec<String> {
        std::fs::read_to_string(self.history_log()).unwrap_or_default().lines().map(str::to_string).collect()
    }

    /// As leituras da história depois de o processo em segundo plano parar de
    /// anotar: a lista só vale quando não muda por meio segundo.
    fn settled_history_reads(&self) -> Vec<String> {
        let mut last = self.history_reads();
        for _ in 0..100 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let now = self.history_reads();
            if now == last {
                return now;
            }
            last = now;
        }
        panic!("the reads of the history never stopped: {last:?}");
    }

    /// As leituras da história depois de esperar até `at_least` delas
    /// chegarem, por um tempo.
    fn history_reads_after_waiting(&self, at_least: usize) -> Vec<String> {
        for _ in 0..200 {
            if self.history_reads().len() >= at_least {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        self.history_reads()
    }

    /// A linha que a leitura da história do mapa do projeto deixa no registro.
    fn model_read_line(&self) -> String {
        format!("history-all {} --out {} --json", self.root.display(), map_store::model_path(&self.root).display())
    }

    /// O binário com `args`, no projeto, com a pasta pessoal falsa, o `gh`
    /// falso à frente do `PATH` e nenhuma sessão nem spec forçada.
    fn command(&self, args: &[&str], stdin: &str) -> Output {
        self.command_in(&self.root, args, stdin)
    }

    /// Como [`Self::command`], rodando de dentro de `dir`.
    fn command_in(&self, dir: &Path, args: &[&str], stdin: &str) -> Output {
        self.command_as(None, dir, args, stdin)
    }

    /// Como [`Self::command_in`], com a sessão `session` no ambiente: numa
    /// conversa, o gancho do terminal passa a sessão dela ao comando.
    fn command_as(&self, session: Option<&str>, dir: &Path, args: &[&str], stdin: &str) -> Output {
        self.command_without(session, dir, args, stdin, &[])
    }

    /// Como [`Self::command_as`], sem as variáveis `unset` no ambiente.
    fn command_without(&self, session: Option<&str>, dir: &Path, args: &[&str], stdin: &str, unset: &[&str]) -> Output {
        let mut binary = binary::isolated(self.rt.as_deref().unwrap_or_else(|| Path::new(env!("CARGO_BIN_EXE_mustard-rt"))));
        self.env(&mut binary);
        if let Some(session) = session {
            binary.env("MUSTARD_SESSION_ID", session);
        }
        for name in unset {
            binary.env_remove(name);
        }
        let mut child = binary
            .args(args)
            .current_dir(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the binary runs");
        if let Some(mut pipe) = child.stdin.take() {
            let _ = pipe.write_all(stdin.as_bytes());
        }
        child.wait_with_output().expect("the binary finishes")
    }

    /// O ambiente de quem roda: a pasta pessoal falsa, o `gh` falso à frente
    /// do `PATH` e nenhuma sessão nem spec forçada.
    fn env<'a>(&self, command: &'a mut Command) -> &'a mut Command {
        let path = format!("{}:{}", self.bin.display(), std::env::var("PATH").unwrap_or_default());
        command
            .env("PATH", path)
            .env("GH_LOG", self.gh_log())
            .env("GH_STATE", &self.remotes)
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("CLAUDE_PROJECT_DIR", &self.root)
            .env("MUSTARD_CLAUDE_BIN", self.home.join("sem-claude"))
            // O programa compilado da versão em construção nasce na pasta
            // pessoal falsa, nunca na pasta de verdade de quem roda a suíte.
            .env("MUSTARD_BUILD_DIR", self.home.join("build"))
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("CLAUDE_PLUGIN_ROOT")
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("MUSTARD_ACTIVE_SPEC")
            .env_remove("TYPESAFE_API_KEY")
            .env_remove("MUSTARD_JEV_URL")
            .env_remove("MUSTARD_SESSION_ID")
            .env_remove("CLAUDE_SESSION_ID")
            .env_remove("CLAUDE_CODE_SESSION_ID")
    }

    /// O agente grava, pelo comando que o pedido ensina, o passo de término
    /// de cada tarefa da onda `wave`: sem ele, a entrega que dá a tarefa como
    /// feita é recusada.
    fn finish_tasks(&self, wave: u64) {
        let log = self.log();
        let codes = log.codes();
        for task in log.visible().into_iter().filter(|e| e.event_type == "task" && e.wave() == Some(wave)) {
            let item = codes.get(&task.id).cloned().unwrap_or_else(|| task.id.to_string());
            self.write("step", &json!({"wave": wave, "item": item, "text": "A tarefa ficou pronta."}));
        }
    }

    /// O agente lê, de dentro da cópia e pelo comando que o pedido ensina,
    /// cada item que o envio da onda `wave` manda ler (`read_items`): sem
    /// isso a entrega é recusada.
    fn read_request(&self, wave: u64) {
        self.read_send(|sent| sent.wave() == Some(wave) && sent.str_field("role") != Some("review"));
    }

    /// O revisor lê, de dentro da cópia dele e pelo comando que o pedido
    /// ensina, cada item que o envio da revisão manda ler: sem isso o
    /// veredito é recusado.
    fn read_review(&self) {
        self.read_send(|sent| sent.str_field("role") == Some("review"));
    }

    /// A leitura, de dentro da cópia do último envio que `pick` escolhe, de
    /// cada item da lista dele.
    fn read_send(&self, pick: impl Fn(&SpecEvent) -> bool) {
        let log = self.log();
        let sent = log
            .visible()
            .into_iter()
            .rfind(|e| e.event_type == "send" && pick(e))
            .unwrap_or_else(|| panic!("nenhum envio gravado para a leitura"));
        let copy = PathBuf::from(sent.str_field("copy").expect("the copy"));
        let root = self.root.display().to_string();
        let listed = log.send_field(sent.id, "read_items").and_then(Value::as_array).cloned().unwrap_or_default();
        for item in listed.iter().filter_map(Value::as_str) {
            let block = format!("item-{item}");
            let lesson = item.strip_prefix("lesson-");
            let mut args = vec!["run", "read", lesson.map_or(block.as_str(), |_| "lessons")];
            args.extend(lesson.into_iter().flat_map(|n| ["--term", n]));
            args.extend(["--root", &root, "--spec", SPEC]);
            let out = self.command_in(&copy, &args, "");
            assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stdout));
        }
    }

    /// Um comando `run`, que precisa responder `ok`.
    fn run(&self, args: &[&str]) -> Value {
        let report = self.answer(args);
        assert_eq!(report["ok"], json!(true), "{args:?}: {report}");
        report
    }

    /// Um comando `run` que pode recusar: a resposta vem como veio.
    fn answer(&self, args: &[&str]) -> Value {
        self.answer_in(None, &self.root, args)
    }

    /// Como [`Self::answer`], rodando de dentro de `dir` e, com `session`,
    /// na conversa dela.
    fn answer_in(&self, session: Option<&str>, dir: &Path, args: &[&str]) -> Value {
        let mut owned: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
        if matches!(args.first(), Some(&"round" | &"close")) {
            let previous = args.windows(2).find(|pair| pair[0] == "--report").map(|pair| pair[1]).unwrap_or_default();
            let report = wave_review::review_report(&self.root, SPEC, previous, |wave| {
                self.answer_in(session, dir, &["review-wave", "--wave", &wave.to_string(), "--spec", SPEC])
            });
            if !report.is_empty() {
                if let Some(at) = owned.iter().position(|arg| arg == "--report") {
                    owned[at + 1] = report;
                } else {
                    owned.extend(["--report".into(), report]);
                }
            }
        }
        let mut all = vec!["run"];
        all.extend(owned.iter().map(String::as_str));
        let out = self.command_as(session, dir, &all, "");
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{args:?} did not answer JSON ({e}): {text}{}", String::from_utf8_lossy(&out.stderr)))
    }

    /// Um comando `run` da conversa da sessão `session`, que pode recusar: a
    /// resposta vem como veio.
    fn answer_as(&self, session: &str, args: &[&str]) -> Value {
        let mut all = vec!["run"];
        all.extend_from_slice(args);
        let out = self.command_as(Some(session), &self.root, &all, "");
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{args:?} did not answer JSON ({e}): {text}{}", String::from_utf8_lossy(&out.stderr)))
    }

    /// Um comando `run` da conversa da sessão `session`, que precisa
    /// responder `ok`.
    fn run_as(&self, session: &str, args: &[&str]) -> Value {
        let report = self.answer_as(session, args);
        assert_eq!(report["ok"], json!(true), "{args:?}: {report}");
        report
    }

    /// A abertura da spec pela conversa da sessão [`SESSION`], como quem
    /// conduz a roda: é ela que fica ligada à spec, e a fala dela entra.
    fn open(&self) -> Value {
        self.run_as(SESSION, &["open", "--kind", "feature", "--name", SPEC, "--base", "dev"])
    }

    /// Uma gravação pelo `run write`.
    fn write(&self, event_type: &str, fields: &Value) -> Value {
        self.run(&["write", event_type, "--spec", SPEC, "--json", &fields.to_string()])
    }

    /// Um evento do harness entregue ao gancho, como a sessão entrega; devolve
    /// o que o gancho disse.
    fn hook(&self, event: &str, payload: &Value) -> String {
        let out = self.command(&["on", event], &payload.to_string());
        assert_eq!(out.status.code(), Some(0), "a hook always exits 0: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    /// Como [`Self::hook`], com o gancho rodando de dentro de `dir` e sem a
    /// pasta do projeto no ambiente: a raiz sai só de onde ele roda e do
    /// `cwd` do evento.
    fn hook_from(&self, dir: &Path, event: &str, payload: &Value) -> String {
        let out = self.command_without(None, dir, &["on", event], &payload.to_string(), &["CLAUDE_PROJECT_DIR"]);
        assert_eq!(out.status.code(), Some(0), "a hook always exits 0: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    /// As chamadas ao `gh` falso, na ordem.
    fn gh_calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.gh_log()).unwrap_or_default().lines().map(str::to_string).collect()
    }

    fn gh_log(&self) -> PathBuf {
        self.home.join("gh.log")
    }

    fn log(&self) -> SpecLog {
        store::read(&store::spec_file(&self.root, SPEC).expect("spec file")).expect("readable").expect("the spec file")
    }

    /// O arquivo de operações da spec: o que o programa observou de si.
    fn operations(&self) -> SpecLog {
        store::operations_file(&self.root, SPEC).and_then(|path| store::read(&path).ok().flatten()).unwrap_or_default()
    }
}

/// A lista `agreed` do veredito final e da entrega da onda, com todo o
/// combinado vigente atendido: estes testes provam o fluxo do fechamento e do
/// pull request, não o do combinado — sem a lista, a revisão final e a
/// entrega da onda, cujo pedido leva as respostas do levantamento, seriam
/// recusadas por faltar item.
fn agreed_all_met(project: &Project) -> Value {
    let log = project.log();
    let codes = log.codes();
    let items: Vec<Value> = mustard_core::domain::wave_prompt::all_agreed(&log)
        .iter()
        .map(|item| json!({"item": codes.get(&item.id).cloned().unwrap_or_default(), "met": true}))
        .collect();
    json!(items)
}

/// A fala do usuário, pelo gancho da entrada; devolve o número dela.
fn user_says(project: &Project, text: &str) -> u64 {
    project.hook(
        "UserPromptSubmit",
        &json!({"hook_event_name": "UserPromptSubmit", "prompt": text, "session_id": SESSION,
            "cwd": project.root.to_string_lossy()}),
    );
    let log = project.log();
    let said = log
        .visible()
        .into_iter()
        .rfind(|e| e.event_type == "message" && e.str_field("author") == Some("user"))
        .expect("the entry hook recorded the message");
    assert_eq!(said.str_field("text"), Some(text));
    said.id
}

/// O levantamento inteiro: o objetivo, o `grill`, que grava os pontos, e
/// cada ponto com os fatos somados, respondido e fechado.
fn survey(project: &Project) {
    survey_with(project, "Combinar o item", GOAL);
}

/// O começo do levantamento: o objetivo de título `title` e texto `goal`, que
/// o usuário diz, o `grill`, que grava os pontos, e cada ponto com os fatos
/// somados, ainda aberto. Devolve o número da fala do objetivo, os pontos do
/// `grill` e o ponto que ficou aberto por último.
fn open_the_points(project: &Project, title: &str, goal: &str) -> (u64, Vec<Value>, Value) {
    let said = user_says(project, goal);
    project.write("context", &json!({"title": title, "agent": "- conferir pelo teste", "text": goal, "origin": said}));
    let grilled = project.run(&["grill", "--spec", SPEC, "--kinds", "feature", "--full"]);
    let points = grilled["points"].as_array().cloned().expect("the point list");
    assert!(!points.is_empty(), "{grilled}");
    let mut current = Value::Null;
    for point in &points {
        let facts = json!([{"text": "A saudação mora no programa.", "source": "src/main.rs:2"}]);
        current = project.write("point", &json!({"replaces": point["id"], "facts": facts}))["point"].clone();
    }
    (said, points, current)
}

/// [`survey`] com o objetivo de título `title` e texto `goal`, que o usuário
/// diz.
fn survey_with(project: &Project, title: &str, goal: &str) {
    let (said, points, mut current) = open_the_points(project, title, goal);
    for point in &points {
        let code = current["code"].as_str().expect("the open point").to_string();
        let answer = project.write(
            "decision",
            &json!({"title": "Combinar o item", "agent": format!("- ponto {code}"), "text": "O usuário respondeu ao ponto.", "keys": ["levantamento"],
                "why": "o usuário respondeu", "origin": said, "applies_to": {"files": ["**"]}}),
        );
        let closed = project.write(
            "point",
            &json!({"block": point["block"], "gap": point["gap"], "from": "gap", "status": "closed",
                "closes": code, "result": [answer["id"]], "origin": said}),
        );
        current = closed["point"].clone();
    }
}

/// O plano de uma tarefa só: o critério com a prova e a tarefa que o cobre,
/// sem onda. A onda nasce da rodada, pelo backlog, com autor binário.
fn plan(project: &Project) {
    plan_files(project, &["src/main.rs"]);
}

/// [`plan`] com a tarefa mudando os arquivos `files`.
fn plan_files(project: &Project, files: &[&str]) {
    let said = user_says(project, "O plano é uma tarefa só, que muda a saudação.");
    let criterion = project.write(
        "criterion",
        &json!({"title": "Combinar o item", "when": "o programa roda", "then": "a saudação nova aparece", "proof": "git --version",
            "form": "ubiquitous", "origin": said}),
    );
    let files: Vec<Value> = files.iter().map(|path| json!({"path": path})).collect();
    project.write(
        "task",
        &json!({"agent": "- conferir pelo teste", "title": "Entregar a tarefa", "text": "Trocar a saudação no programa.", "files": files, "depends_on": [],
            "covers": [criterion["id"]], "origin": said}),
    );
    let planned = project.run(&["plan", "--spec", SPEC]);
    assert_eq!(State::from_log(&project.log()).phase, Some("plan"), "{planned}");
}

/// A primeira rodada: as respostas do levantamento valem para o projeto todo
/// e vão no pedido por padrão, sem Jev, e a rodada solta a onda na mesma
/// chamada, sem pedir escolha a quem conduz nem agente nenhum. Devolve a
/// resposta da rodada.
fn first_round(project: &Project) -> Value {
    let sent = project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert_eq!(sent["dispatch"][0]["wave"], json!(1), "{sent}");
    assert!(sent.get("analysis").is_none(), "{sent}");
    sent
}

/// O clique em "Aprovar" na pergunta da aprovação, pelo gancho da testemunha.
fn approve(project: &Project) {
    let question = translate("approval.question", Locale::PtBr);
    let yes = translate("approval.option", Locale::PtBr);
    project.hook(
        "PostToolUse",
        &json!({
            "hook_event_name": "PostToolUse",
            "tool_name": "AskUserQuestion",
            "tool_input": {"questions": [{"question": question, "options": [{"label": yes}, {"label": "Ajustar"}]}]},
            "tool_response": {"answers": {question: yes}},
            "session_id": SESSION,
            "cwd": project.root.to_string_lossy(),
        }),
    );
    assert_eq!(State::from_log(&project.log()).phase, Some("approved"));
}

/// Quantas chamadas cada passo do fluxo gravou no arquivo de operações da
/// spec, e quantos recibos de leitura a spec guarda (`read`). A leitura, a
/// escrita e as buscas pela linha de comando também gravam a chamada delas
/// ali, e não são passo.
fn calls(project: &Project) -> BTreeMap<String, usize> {
    const TRANSPORT: [&str; 4] = ["read", "write", "search", "knowledge"];
    let (log, operations) = (project.log(), project.operations());
    let receipts = log.visible().into_iter().filter(|e| e.event_type == "call" && e.fields.contains_key("request"));
    let steps = operations.visible().into_iter().filter(|e| e.event_type == "call" && !TRANSPORT.contains(&e.str_field("command").unwrap_or_default()));
    let mut out = BTreeMap::new();
    for call in receipts.chain(steps) {
        *out.entry(call.str_field("command").unwrap_or_default().to_string()).or_insert(0) += 1;
    }
    out
}

/// Uma spec de teste roda de ponta a ponta com o binário novo: abrir,
/// levantar, conferir o plano, aprovar pelo clique, uma rodada, o agente de
/// teste dedicado e o pull request funcionam em sequência, e a pasta da spec
/// termina com três arquivos. Cada passo do fluxo é uma chamada só: abrir 1,
/// levantamento 1 (as respostas gravadas não contam), plano 1, aprovar 0,
/// cada rodada 1 e pull request 1; a escolha antes do envio da primeira onda
/// é uma rodada a mais, a que traz a escolha do orquestrador, e o fechamento
/// é duas chamadas — o pedido do agente de teste dedicado e a aprovação dele
/// —, mesmo com uma onda só. A onda tem uma tarefa só e ainda assim ganha
/// cópia separada, como qualquer outra: o orquestrador edita na cópia, e a
/// rodada leva a mudança de volta ao checkout principal.
#[test]
fn a_test_spec_runs_end_to_end_one_call_per_step_and_leaves_three_files() {
    let project = Project::new();

    let opened = project.open();
    assert_eq!(opened["step"], json!("ask_goal"), "{opened}");
    survey(&project);
    plan(&project);
    approve(&project);

    // Primeira rodada: a análise antes do envio, e a onda ganha cópia
    // separada, mesmo com uma tarefa só.
    let first = first_round(&project);
    let dispatched = first["dispatch"].as_array().cloned().unwrap_or_default();
    assert_eq!(dispatched.len(), 1, "{first}");
    let next = first["next"].as_str().unwrap_or_default();
    assert!(next.contains(translate("round.next", Locale::PtBr)), "{next}");
    let log = project.log();
    let sent = log.visible().into_iter().rfind(|e| e.event_type == "send").expect("the send");
    let copy = PathBuf::from(sent.str_field("copy").expect("the copy"));

    // A onda muda o arquivo na cópia dela e grava a entrega na spec; a
    // rodada assume a volta e não pede revisão nenhuma dela.
    std::fs::write(copy.join("src/main.rs"), "fn main() {\n    println!(\"olá\");\n}\n").expect("the change");
    let delivered = json!({"wave": 1, "text": "A saudação virou olá.", "files": ["src/main.rs"],
        "commit": "a saudação vira olá", "agreed": agreed_all_met(&project)});
    project.read_request(1);
    project.finish_tasks(1);
    project.run(&["write", "delivered", "--spec", SPEC, "--json", &delivered.to_string()]);
    let second = project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert!(second.get("reviews").is_none(), "{second}");
    assert!(copy.join(".git").is_file(), "the copy stays for the next wave once the round takes the change back: {second}");
    assert_eq!(std::fs::read_to_string(project.root.join("src/main.rs")).unwrap(), "fn main() {\n    println!(\"olá\");\n}\n");

    // O fechamento roda o lint e o critério e pede o agente de teste
    // dedicado, mesmo numa spec de uma onda.
    let asked = project.run(&["close", "--spec", SPEC]);
    assert_eq!(asked["phase"], json!("running"), "{asked}");
    assert_eq!(asked["review"]["final"], json!(true), "{asked}");
    // O pedido de verdade que o binário monta para o revisor usa o nome de
    // mercado dos termos, sem sobra do nome antigo.
    let log = project.log();
    let review = log.visible().into_iter().rfind(|e| e.event_type == "send" && e.str_field("role") == Some("review")).expect("the review send");
    let request = review.str_field("text").unwrap_or_default();
    assert!(request.contains("## Requisitos acordados"), "o pedido da revisão usa o nome de mercado: {request}");
    assert!(
        !request.contains("Combinado") && !request.contains("## Prova") && !request.contains("## Revisão final"),
        "o pedido da revisão não guarda um nome antigo: {request}"
    );

    // O revisor grava o veredito aprovado; o fechamento o assume e fecha.
    let verdict = json!({"final": true, "result": "approved", "text": "A saudação mudou.",
        "agreed": agreed_all_met(&project)});
    project.read_review();
    project.run(&["write", "verdict", "--spec", SPEC, "--json", &verdict.to_string()]);
    let closed = project.run(&["close", "--spec", SPEC]);
    assert_eq!(closed["phase"], json!("closed"), "{closed}");
    assert!(closed.get("review").is_none(), "{closed}");
    assert!(!copy.exists() && !copy.parent().expect("the folder of the work").exists(), "closing removes the copies of the work: {closed}");
    let pr_line = format!("mustard-rt run pr-open --base dev --head feature/{SPEC} --spec {SPEC}");
    assert_eq!(closed["command"], json!(pr_line), "{closed}");

    // O pull request abre pela linha que o fechamento devolveu.
    let argv: Vec<&str> = pr_line.split_whitespace().skip(2).collect();
    let pr = project.run(&argv);
    assert_eq!(pr["number"], json!(7), "{pr}");
    let asked_gh = std::fs::read_to_string(project.gh_log()).expect("the fake gh was called");
    assert!(asked_gh.lines().any(|l| l.starts_with("pr create") && l.contains("--head feature/ponta")), "{asked_gh}");
    let state = State::from_log(&project.log());
    assert_eq!(state.phase, Some("pr_open"), "{pr}");

    let expected: BTreeMap<String, usize> =
        [("open", 1), ("grill", 1), ("plan", 1), ("review-wave", 1), ("round", 2), ("close", 2), ("pr-open", 1)]
            .into_iter()
            .map(|(command, count)| (command.to_string(), count))
            .collect();
    // A leitura que o agente faz do pedido dele é dele, e não um passo do fluxo.
    let mut seen = calls(&project);
    assert!(seen.remove("read").is_some_and(|reads| reads > 0), "the agent read what its request lists: {seen:?}");
    assert_eq!(seen, expected, "each flow step is one call, with one native diff review and closing two");

    let folder = project.root.join(".claude/spec").join(SPEC);
    let mut names: Vec<String> = std::fs::read_dir(&folder)
        .expect("the spec folder")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    // A pasta da spec guarda o estado, e mais nada: o retrato do mapa da base
    // mora fora do projeto, e o fechamento o apaga.
    assert_eq!(names, ["spec.ndjson"], "the spec keeps its native events without automatic publication");
    assert!(!mustard_core::io::wave_prompt::validation_base_map(&project.root, SPEC).exists(), "o fechamento apagou o retrato do mapa da base");

    // O fluxo inteiro não grava onda pela linha de comando: toda linha de onda
    // do arquivo da spec — lida crua, com as versões antigas e as removidas —
    // tem autor binário, e há ao menos uma, para a conferência não passar num
    // arquivo sem onda.
    let path = store::spec_file(&project.root, SPEC).expect("spec file");
    let content = std::fs::read_to_string(&path).expect("the spec file");
    let waves: Vec<Value> = content
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|event| event["type"] == json!("wave"))
        .collect();
    assert!(!waves.is_empty(), "the round wrote the wave of the lot: {content}");
    for wave in &waves {
        assert_eq!(wave["author"], json!("binary"), "a wave that is not the binary's: {wave}");
    }
}

/// A spec de uma onda, da abertura ao fechamento, com a mudança entregue: devolve
/// os argumentos do `pr-open` que o fechamento apontou.
fn closed_with_one_wave(project: &Project) -> Vec<String> {
    closed_with_goal(project, "Combinar o item", GOAL)
}

/// [`closed_with_one_wave`] com o objetivo de título `title` e texto `goal`.
fn closed_with_goal(project: &Project, title: &str, goal: &str) -> Vec<String> {
    closed_by(project, title, goal, &|args| project.run(args))
}

/// [`closed_with_goal`] com as duas chamadas do `close` feitas por `closing`:
/// é por ela que um teste fecha a spec de dentro de uma pasta e de uma
/// conversa.
fn closed_by(project: &Project, title: &str, goal: &str, closing: &dyn Fn(&[&str]) -> Value) -> Vec<String> {
    project.open();
    survey_with(project, title, goal);
    plan(project);
    approve(project);
    let first = first_round(project);
    assert_eq!(first["dispatch"].as_array().map(Vec::len), Some(1), "{first}");
    let log = project.log();
    let sent = log.visible().into_iter().rfind(|e| e.event_type == "send").expect("the send");
    let copy = PathBuf::from(sent.str_field("copy").expect("the copy"));
    std::fs::write(copy.join("src/main.rs"), "fn main() {\n    println!(\"olá\");\n}\n").expect("the change");
    let delivered = json!({"wave": 1, "text": "A saudação virou olá.", "files": ["src/main.rs"],
        "commit": "a saudação vira olá", "agreed": agreed_all_met(project)});
    project.read_request(1);
    project.finish_tasks(1);
    project.run(&["write", "delivered", "--spec", SPEC, "--json", &delivered.to_string()]);
    project.run(&["round", "--dispatch", "--spec", SPEC]);
    closing(&["close", "--spec", SPEC]);
    let verdict = json!({"final": true, "result": "approved", "text": "A saudação mudou.",
        "agreed": agreed_all_met(project)});
    project.read_review();
    project.run(&["write", "verdict", "--spec", SPEC, "--json", &verdict.to_string()]);
    let closed = closing(&["close", "--spec", SPEC]);
    let line = closed["command"].as_str().expect("the pr-open line").to_string();
    line.split_whitespace().skip(2).map(str::to_string).collect()
}

/// O revisor final, lançado pelo gancho com o JSON do Claude Code, sai no
/// modelo que a chamada pede e no esforço de sempre, e o envio da revisão
/// grava o modelo que valeu; sem modelo na chamada, sai o do `mustard.json`,
/// e o envio grava esse. O veredito aprovado da versão nova do envio ainda
/// fecha a spec.
#[test]
fn the_final_reviewer_keeps_the_model_the_call_asks_and_the_send_records_it() {
    let project = Project::new();
    deliver_the_first_wave(&project);
    project.run(&["round", "--dispatch", "--spec", SPEC]);
    let asked = project.run(&["close", "--spec", SPEC]);
    assert_eq!(asked["review"]["final"], json!(true), "{asked}");
    let read = asked["review"]["read"].as_str().expect("the command that reads the request").to_string();
    let launch = |model: Option<&str>| -> Value {
        let mut input = json!({"subagent_type": "mustard-review", "description": "revisão final", "prompt": read});
        if let Some(model) = model {
            input["model"] = json!(model);
        }
        let payload = json!({"hook_event_name": "PreToolUse", "tool_name": "Agent", "session_id": SESSION,
            "cwd": project.root.to_string_lossy(), "tool_input": input});
        let out = project.hook("PreToolUse", &payload);
        let said: Value = serde_json::from_str(&out).unwrap_or_else(|_| panic!("the hook answers JSON: {out}"));
        said["hookSpecificOutput"]["updatedInput"].clone()
    };
    let recorded = || {
        let log = project.log();
        let sent = log.visible().into_iter().rfind(|e| e.event_type == "send" && e.str_field("role") == Some("review")).expect("the review send");
        (log.send_field(sent.id, "model_used").and_then(Value::as_str).map(str::to_string), log.send_field(sent.id, "model").and_then(Value::as_str).map(str::to_string))
    };

    let asked_opus = launch(Some("opus"));
    assert_eq!((asked_opus["model"].as_str(), asked_opus["effort"].as_str()), (Some("opus"), Some("xhigh")), "{asked_opus}");
    assert_eq!(recorded(), (Some("opus".into()), Some("sonnet".into())), "the send keeps what was configured and records what held");

    let unasked = launch(None);
    assert_eq!((unasked["model"].as_str(), unasked["effort"].as_str()), (Some("sonnet"), Some("xhigh")), "{unasked}");
    assert_eq!(recorded(), (Some("sonnet".into()), Some("sonnet".into())), "a call without a model goes on the configured one");

    let verdict = json!({"final": true, "result": "approved", "text": "A saudação mudou.", "agreed": agreed_all_met(&project)});
    project.read_review();
    project.run(&["write", "verdict", "--spec", SPEC, "--json", &verdict.to_string()]);
    let closed = project.run(&["close", "--spec", SPEC]);
    assert_eq!(closed["phase"], json!("closed"), "{closed}");
}

/// O pedido da onda e o do revisor final, lidos pelo comando que o agente
/// roda, dizem nos dois idiomas que a regra do projeto sobre idioma vence a
/// linha de idiomas do pedido: uma vez na própria linha e outra na seção
/// das regras do projeto, que leva o `CLAUDE.md` da raiz. Sem isso o agente
/// segue o idioma do código contra a regra local, e o revisor cobra o que a
/// regra manda.
#[test]
fn the_wave_and_final_review_requests_say_a_project_rule_on_language_wins_over_the_languages_line() {
    for (text, on_the_line, on_the_rules) in [
        ("pt-BR", "Regra do projeto que trate de idioma vence esta linha.", "onde ele trata de idioma, vence a linha de idiomas do pedido"),
        ("en-US", "A project rule that deals with language overrides this line.", "where it deals with language, it overrides the request's languages line"),
    ] {
        let project = Project::new();
        let config = std::fs::read_to_string(project.root.join("mustard.json")).expect("the config");
        let mut config: Value = serde_json::from_str(&config).expect("the config is JSON");
        config["language"]["text"] = json!(text);
        std::fs::write(project.root.join("mustard.json"), config.to_string()).expect("the config");
        std::fs::write(project.root.join("CLAUDE.md"), "# Regras\n\n- Títulos de describe e it em português.\n").expect("the rules");
        git(&project.root, &["add", "CLAUDE.md"]);
        git(&project.root, &["commit", "-q", "-m", "regras"]);

        deliver_the_first_wave(&project);
        project.run(&["round", "--dispatch", "--spec", SPEC]);
        let asked = project.run(&["close", "--spec", SPEC]);
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");

        for block in ["request-1", "request-review"] {
            let out = project.command(&["run", "read", block, "--spec", SPEC], "");
            assert!(out.status.success(), "{text} {block}: {}", String::from_utf8_lossy(&out.stdout));
            let request = String::from_utf8_lossy(&out.stdout);
            assert!(request.contains("Títulos de describe e it em português."), "{text} {block} carries the rules file: {request}");
            assert_eq!(request.matches(on_the_line).count(), 1, "{text} {block}: the languages line says the rule wins: {request}");
            assert_eq!(request.matches(on_the_rules).count(), 1, "{text} {block}: the rules section says it wins: {request}");
        }
    }
}

/// O `pr-open` envia a branch da spec ao servidor antes de abrir o pull
/// request: com o servidor recusando o recebimento, o pedido não nasce (o
/// provedor nem é chamado), a resposta traz o `push:` com a mensagem do git e a
/// spec segue fechada; com o servidor aceitando, o mesmo comando, de novo,
/// leva a branch, abre o pedido e a spec passa a pull request aberto. Um
/// commit novo na branch chega ao servidor no `pr-open` seguinte.
#[test]
fn a_pr_open_sends_the_branch_first_and_a_refused_send_opens_nothing() {
    let project = Project::new();
    let server = project.remotes.join("projeto.git");
    let refuse = server.join("hooks/pre-receive");
    executable::write_executable(&refuse, "#!/bin/sh\necho 'o servidor recusou o envio' >&2\nexit 1\n");
    let line = closed_with_one_wave(&project);
    let argv: Vec<&str> = line.iter().map(String::as_str).collect();

    let asked_before = project.gh_calls();
    let refused = project.answer(&argv);
    assert_eq!(refused["ok"], json!(false), "{refused}");
    let error = refused["error"].as_str().unwrap_or_default().to_string();
    assert!(error.starts_with("push: ") && error.contains("o servidor recusou o envio"), "{refused}");
    assert_eq!(project.gh_calls(), asked_before, "the provider was not asked anything by the refused open");
    assert!(git_out(&server, &["for-each-ref", &format!("refs/heads/{BRANCH}")]).is_empty(), "nothing reached the server");
    assert_eq!(State::from_log(&project.log()).phase, Some("closed"), "{refused}");

    std::fs::remove_file(&refuse).expect("the server hook leaves");
    let opened = project.answer(&argv);
    assert_eq!(opened["ok"], json!(true), "{opened}");
    assert_eq!(opened["number"], json!(7), "{opened}");
    assert_eq!(
        git_out(&server, &["rev-parse", &format!("refs/heads/{BRANCH}")]),
        git_out(&project.root, &["rev-parse", "HEAD"]),
        "the branch of the spec is on the server"
    );
    assert!(project.gh_calls().iter().any(|call| call.starts_with("pr create")), "{:?}", project.gh_calls());
    assert!(!asked_before.iter().any(|call| call.starts_with("pr create")), "{asked_before:?}");
    assert_eq!(State::from_log(&project.log()).phase, Some("pr_open"), "{opened}");

    // O commit que vem depois chega ao servidor no `pr-open` seguinte.
    git(&project.root, &["commit", "-q", "--allow-empty", "-m", "o ajuste depois do pedido"]);
    let again = project.answer(&argv);
    assert_eq!(again["ok"], json!(true), "{again}");
    assert_eq!(
        git_out(&server, &["rev-parse", &format!("refs/heads/{BRANCH}")]),
        git_out(&project.root, &["rev-parse", "HEAD"]),
        "the new commit is on the server"
    );
}

/// O `gh` falso do pull request 7 aberto e fora de rascunho: anota cada
/// chamada e responde a consulta dele. Qualquer outro pedido falha, e fica
/// anotado do mesmo jeito.
const OPEN_PR_GH: &str = r#"#!/bin/sh
echo "$*" >> "$GH_LOG"
case "$1 $2" in
"pr view")
  echo '{"number":7,"title":"t","state":"OPEN","headRefName":"feature/ponta","baseRefName":"dev","isDraft":false,"url":"https://github.com/exemplo/projeto/pull/7"}'
  exit 0 ;;
esac
exit 1
"#;

/// A obra fechada abre o pull request, e o usuário pede um ajuste: a
/// reabertura volta a spec à execução e só pergunta ao provedor se o pull
/// request já entrou na base. Nada o põe em rascunho, e a resposta não fala em
/// rascunho. O merge pelo Mustard é recusado enquanto a spec não fecha de novo,
/// sem perguntar as verificações ao provedor nem juntar nada.
#[test]
fn reopening_leaves_the_open_pull_request_out_of_draft_and_the_merge_waits_for_the_close() {
    let project = Project::new();
    let line = closed_with_one_wave(&project);
    let argv: Vec<&str> = line.iter().map(String::as_str).collect();
    let opened = project.run(&argv);
    assert_eq!(opened["number"], json!(7), "{opened}");
    executable::write_executable(&project.bin.join("gh"), OPEN_PR_GH);

    let asked_before = project.gh_calls().len();
    let reopened = project.run(&["reopen", "--spec", SPEC, "--reason", "Trocar a saudação de novo."]);
    assert_eq!((reopened["from"].clone(), reopened["phase"].clone()), (json!("pr_open"), json!("running")), "{reopened}");
    assert!(reopened.get("draft").is_none() && reopened.get("warnings").is_none(), "{reopened}");
    let next = reopened["next"].as_str().unwrap_or_default();
    assert!(next.contains(translate("reopen.same_pr", Locale::PtBr)), "{next}");
    let asked = project.gh_calls()[asked_before..].to_vec();
    assert_eq!(asked.len(), 1, "only whether the pull request is already merged: {asked:?}");
    assert!(asked[0].starts_with("pr view 7 "), "{asked:?}");
    assert_eq!(State::from_log(&project.log()).phase, Some("running"));

    let asked_before = project.gh_calls().len();
    let merge = project.answer(&["pr-merge", "--pr", "7"]);
    assert_eq!(merge["ok"], json!(false), "{merge}");
    assert_eq!((merge["action"].clone(), merge["reason"].clone()), (json!("refused"), json!("spec-not-closed")), "{merge}");
    assert_eq!(merge["checks"], json!("not-asked"), "{merge}");
    let hint = merge["hint"].as_str().unwrap_or_default();
    assert!(hint.contains("foi reaberta e ainda não fechou de novo") && hint.contains("#7"), "{hint}");
    let asked = project.gh_calls()[asked_before..].to_vec();
    assert_eq!(asked.len(), 1, "only which pull request it is: {asked:?}");
    assert!(asked[0].starts_with("pr view 7 "), "{asked:?}");
    assert_eq!(State::from_log(&project.log()).phase, Some("running"), "nothing was merged");
}

/// O `gh` falso que guarda o corpo do pull request 7 como o provedor guarda:
/// o `pr create` e o PATCH do corpo copiam o arquivo que recebem, a consulta
/// só acha o pull request depois de criado, e o `gh api -i` devolve os
/// cabeçalhos e o documento com o corpo guardado, escapado para JSON. O PATCH
/// do título passa. Cada chamada fica anotada; qualquer outro pedido falha.
const STORED_BODY_GH: &str = r#"#!/bin/sh
echo "$*" >> "$GH_LOG"
body="$GH_STATE/pr7-body.md"
case "$*" in
"pr create "*)
  while [ $# -gt 0 ]; do [ "$1" = "--body-file" ] && cp "$2" "$body"; shift; done
  echo 'https://github.com/exemplo/projeto/pull/7'
  exit 0 ;;
"pr view "*)
  [ -f "$body" ] || { echo 'no pull requests found' >&2; exit 1; }
  echo '{"number":7,"title":"t","state":"OPEN","headRefName":"feature/ponta","baseRefName":"dev","isDraft":false,"url":"https://github.com/exemplo/projeto/pull/7"}'
  exit 0 ;;
"api -i repos/{owner}/{repo}/pulls/7")
  printf 'HTTP/2.0 200 OK\r\nEtag: W/"e"\r\n\r\n{"number":7,"title":"t","body":"'
  sed -e 's/\\/\\\\/g' -e 's/"/\\"/g' "$body" | awk 'NR > 1 { printf "%s", "\\n" } { printf "%s", $0 }'
  printf '"}\n'
  exit 0 ;;
"api --method PATCH repos/{owner}/{repo}/pulls/7 -F body=@"*)
  cp "${6#body=@}" "$body"
  exit 0 ;;
"api --method PATCH repos/{owner}/{repo}/pulls/7 -f title="*) exit 0 ;;
esac
exit 1
"#;

/// O `gh` falso de um pull request que nasce no `pr create`, o 7: anota cada
/// chamada e, depois de criado, responde a consulta dele aberto e as
/// verificações com o que o teste deixa em `checks`. Qualquer outro pedido
/// falha, e fica anotado do mesmo jeito.
const OPENED_PR_GH: &str = r#"#!/bin/sh
echo "$*" >> "$GH_LOG"
case "$1 $2" in
"pr create")
  touch "$GH_STATE/open"
  echo 'https://github.com/exemplo/projeto/pull/7'
  exit 0 ;;
"pr view")
  [ -f "$GH_STATE/open" ] || { echo 'no pull requests found' >&2; exit 1; }
  case "$*" in *statusCheckRollup*) cat "$GH_STATE/checks"; exit $? ;; esac
  echo '{"number":7,"title":"t","state":"OPEN","headRefName":"feature/ponta","baseRefName":"dev","isDraft":false,"url":"https://github.com/exemplo/projeto/pull/7"}'
  exit 0 ;;
esac
exit 1
"#;

/// O corpo do pull request que alguém editou no provedor fica. A obra fechada
/// abre o pull request com o corpo gerado; o `pr-open` seguinte lê o corpo
/// pelo `gh api -i`, vê que ele segue igual ao gerado e o refaz. Depois que
/// alguém muda a descrição no provedor, o `pr-open` seguinte lê o corpo
/// editado, não pede a edição do corpo — só a do título —, responde com
/// `bodyKept` e o aviso, e a edição segue no provedor.
#[test]
fn a_pull_request_body_edited_on_the_provider_is_kept_by_the_next_pr_open() {
    let project = Project::new();
    let line = closed_with_one_wave(&project);
    let argv: Vec<&str> = line.iter().map(String::as_str).collect();
    executable::write_executable(&project.bin.join("gh"), STORED_BODY_GH);
    let stored = project.remotes.join("pr7-body.md");
    let body_edit = "api --method PATCH repos/{owner}/{repo}/pulls/7 -F body=@";
    let title_edit = "api --method PATCH repos/{owner}/{repo}/pulls/7 -f title=";
    let body_read = "api -i repos/{owner}/{repo}/pulls/7";

    let opened = project.run(&argv);
    assert_eq!((opened["action"].clone(), opened["number"].clone()), (json!("open"), json!(7)), "{opened}");
    let generated = std::fs::read_to_string(&stored).expect("the provider keeps the body it got");

    // Ninguém mexeu no corpo: ele é lido, segue igual ao gerado e é refeito.
    let asked_before = project.gh_calls().len();
    let rewritten = project.run(&argv);
    assert_eq!((rewritten["action"].clone(), rewritten.get("bodyKept")), (json!("edit"), None), "{rewritten}");
    let asked = project.gh_calls()[asked_before..].to_vec();
    assert!(asked.iter().any(|call| call == body_read), "{asked:?}");
    assert!(asked.iter().any(|call| call.starts_with(body_edit)), "the untouched body is rewritten: {asked:?}");
    assert_eq!(std::fs::read_to_string(&stored).unwrap(), generated, "the rewrite carries the same generated body");

    // Alguém acrescenta uma nota à descrição no provedor; o comentário
    // escondido do fim fica onde estava.
    let (text, mark) = generated.rsplit_once("<!-- mustard-body:").expect("the generated body carries its mark");
    let edited = format!("{text}Nota de quem revisa: conferir a saudação no terminal.\n\n<!-- mustard-body:{mark}");
    std::fs::write(&stored, &edited).expect("the edit on the provider");

    let asked_before = project.gh_calls().len();
    let kept = project.run(&argv);
    assert_eq!(
        (kept["action"].clone(), kept["number"].clone(), kept["bodyKept"].clone()),
        (json!("edit"), json!(7), json!("body-edited")),
        "{kept}"
    );
    let warning = kept["warning"].as_str().unwrap_or_default();
    let body_kept = translate("pr.body_kept", Locale::PtBr).replace("{pr}", "7").replace("{reason}", "body-edited");
    assert!(warning.contains(&body_kept), "{warning}");
    let asked = project.gh_calls()[asked_before..].to_vec();
    assert!(asked.iter().any(|call| call == body_read), "the body is read before anything is written: {asked:?}");
    assert!(!asked.iter().any(|call| call.starts_with(body_edit)), "the edited body is not rewritten: {asked:?}");
    assert!(asked.iter().any(|call| call.starts_with(title_edit)), "the title still follows the spec: {asked:?}");
    assert_eq!(std::fs::read_to_string(&stored).unwrap(), edited, "the edit stays on the provider");
    assert_eq!(State::from_log(&project.log()).phase, Some("pr_open"), "{kept}");
}

/// O corpo sem a marca, de um pull request aberto antes de o Mustard marcar o
/// corpo que gera, fica com um motivo próprio. A obra fechada abre o pull
/// request; a descrição no provedor passa a ser uma sem a marca, e o
/// `pr-open` seguinte lê o corpo, não pede a edição dele — só a do título —,
/// responde `bodyKept` com `body-unmarked` e o aviso desse motivo, e a
/// descrição segue no provedor.
#[test]
fn a_pull_request_body_without_the_mark_is_kept_with_its_own_reason() {
    let project = Project::new();
    let line = closed_with_one_wave(&project);
    let argv: Vec<&str> = line.iter().map(String::as_str).collect();
    executable::write_executable(&project.bin.join("gh"), STORED_BODY_GH);
    let stored = project.remotes.join("pr7-body.md");
    let opened = project.run(&argv);
    assert_eq!((opened["action"].clone(), opened["number"].clone()), (json!("open"), json!(7)), "{opened}");
    let unmarked = "## Objetivo\n\nTrocar a saudação do programa.\n";
    std::fs::write(&stored, unmarked).expect("a description without the mark on the provider");

    let asked_before = project.gh_calls().len();
    let kept = project.run(&argv);
    assert_eq!(
        (kept["action"].clone(), kept["number"].clone(), kept["bodyKept"].clone()),
        (json!("edit"), json!(7), json!("body-unmarked")),
        "{kept}"
    );
    let warning = kept["warning"].as_str().unwrap_or_default();
    let said = translate("pr.body_unmarked", Locale::PtBr).replace("{pr}", "7").replace("{reason}", "body-unmarked");
    assert!(warning.contains(&said), "{warning}");
    let asked = project.gh_calls()[asked_before..].to_vec();
    assert!(asked.iter().any(|call| call == "api -i repos/{owner}/{repo}/pulls/7"), "the body is read: {asked:?}");
    assert!(
        !asked.iter().any(|call| call.starts_with("api --method PATCH repos/{owner}/{repo}/pulls/7 -F body=@")),
        "the unmarked body is not rewritten: {asked:?}"
    );
    assert!(asked.iter().any(|call| call.starts_with("api --method PATCH repos/{owner}/{repo}/pulls/7 -f title=")), "{asked:?}");
    assert_eq!(std::fs::read_to_string(&stored).unwrap(), unmarked, "the description stays on the provider");
}

/// A esteira do pull request aparece na resposta do `pr-open` e na retomada
/// da spec com o pull request aberto, cada vez com o que o provedor responde
/// naquela hora: rodando logo depois de abrir, vermelha numa retomada — com a
/// porta de conserto da spec na frase — e verde na seguinte.
#[test]
fn the_pipeline_of_the_pull_request_is_read_after_the_open_and_on_resume() {
    let project = Project::new();
    executable::write_executable(&project.bin.join("gh"), OPENED_PR_GH);
    let line = closed_with_one_wave(&project);
    let argv: Vec<&str> = line.iter().map(String::as_str).collect();
    let pipeline = |status: &str, conclusion: &str| {
        let rows = json!({"statusCheckRollup": [{"__typename": "CheckRun", "name": "ci", "status": status, "conclusion": conclusion}]});
        std::fs::write(project.remotes.join("checks"), rows.to_string()).expect("the checks");
    };
    let said = |key: &str| translate(key, Locale::PtBr).replace("{pr}", "7").replace("{spec}", SPEC);

    pipeline("IN_PROGRESS", "");
    let opened = project.run(&argv);
    assert_eq!(opened["number"], json!(7), "{opened}");
    assert_eq!(opened["checks"], json!({"state": "running", "text": said("pr.checks.running")}), "{opened}");
    assert!(project.gh_calls().iter().any(|call| call == "pr view 7 --json statusCheckRollup"), "{:?}", project.gh_calls());

    pipeline("COMPLETED", "FAILURE");
    let red = project.run(&["resume", "--spec", SPEC]);
    assert_eq!(red["phase"], json!("pr_open"), "{red}");
    let fix = format!("{} {}", said("pr.checks.failed"), said("pr.checks.fix"));
    assert_eq!(red["checks"], json!({"state": "failed", "text": fix}), "{red}");
    assert!(fix.contains(&format!("mustard-rt run reopen --spec {SPEC} --fix")), "{fix}");

    pipeline("COMPLETED", "SUCCESS");
    let green = project.run(&["resume", "--spec", SPEC]);
    assert_eq!(green["checks"], json!({"state": "passed", "text": said("pr.checks.passed")}), "{green}");
}

/// A rodada que refaz o corpo do pull request aberto deixa o que alguém
/// editou no provedor. A obra fechada abre o pull request pelo `pr-open`,
/// alguém acrescenta uma nota à descrição e o usuário reabre a spec; a rodada
/// seguinte lê o corpo pelo `gh api -i`, não pede a edição dele, e a resposta
/// traz o pull request com o corpo mantido e o aviso. A nota segue no
/// provedor.
#[test]
fn an_edited_pull_request_body_is_kept_by_the_round_after_a_reopen() {
    let project = Project::new();
    let line = closed_with_one_wave(&project);
    let argv: Vec<&str> = line.iter().map(String::as_str).collect();
    executable::write_executable(&project.bin.join("gh"), STORED_BODY_GH);
    let stored = project.remotes.join("pr7-body.md");
    let opened = project.run(&argv);
    assert_eq!((opened["action"].clone(), opened["number"].clone()), (json!("open"), json!(7)), "{opened}");

    let generated = std::fs::read_to_string(&stored).expect("the provider keeps the body it got");
    let (text, mark) = generated.rsplit_once("<!-- mustard-body:").expect("the generated body carries its mark");
    let edited = format!("{text}Nota de quem revisa: conferir a saudação no terminal.\n\n<!-- mustard-body:{mark}");
    std::fs::write(&stored, &edited).expect("the edit on the provider");
    let reopened = project.run(&["reopen", "--spec", SPEC, "--reason", "Trocar a saudação de novo."]);
    assert_eq!(reopened["phase"], json!("running"), "{reopened}");

    let asked_before = project.gh_calls().len();
    let round = project.run(&["round", "--dispatch", "--spec", SPEC]);
    let body_kept = translate("pr.body_kept", Locale::PtBr).replace("{pr}", "7").replace("{reason}", "body-edited");
    assert_eq!(round["pr"], json!({"number": 7, "body": "kept", "warning": body_kept}), "{round}");
    let asked = project.gh_calls()[asked_before..].to_vec();
    assert!(asked.iter().any(|call| call == "api -i repos/{owner}/{repo}/pulls/7"), "the round reads the body on the provider: {asked:?}");
    assert!(
        !asked.iter().any(|call| call.starts_with("api --method PATCH repos/{owner}/{repo}/pulls/7 -F body=@")),
        "the round does not rewrite the edited body: {asked:?}"
    );
    assert_eq!(std::fs::read_to_string(&stored).unwrap(), edited, "the edit stays on the provider");
}

/// O `title` do objetivo dá nome ao pull request e à linha da spec no resumo
/// do projeto, mesmo com uma primeira frase acima do teto: a spec vai da
/// abertura ao fechamento, o `pr-open` abre o pedido com o título, o servidor
/// junta a branch na base, a entrega fora da porta do pull request grava a
/// linha do resumo e a visão geral do painel a mostra com o título.
#[test]
fn the_goal_title_names_the_pull_request_and_the_project_summary() {
    let title = "Saudação nova no programa";
    let goal = "Trocar a saudação do programa por uma frase nova que diga olá a quem abre o terminal.";
    assert!(goal.chars().count() > 60, "the first sentence alone would be refused");
    let project = Project::new();
    let line = closed_with_goal(&project, title, goal);
    let argv: Vec<&str> = line.iter().map(String::as_str).collect();

    let opened = project.run(&argv);
    assert_eq!(opened["number"], json!(7), "{opened}");
    let created = project.gh_calls().into_iter().find(|call| call.starts_with("pr create")).expect("the pull request was created");
    assert!(created.starts_with(&format!("pr create --title {title} --body-file ")), "{created}");

    // O servidor junta o pull request na base com um commit de merge, como o
    // provedor faz: a base antes da spec é o primeiro pai, a branch o segundo.
    let tree = git_out(&project.root, &["rev-parse", &format!("{BRANCH}^{{tree}}")]);
    let merge = git_out(&project.root, &["commit-tree", &tree, "-p", "dev", "-p", BRANCH, "-m", "Merge do pull request #7"]);
    git(&project.root, &["push", "-q", "origin", &format!("{merge}:refs/heads/dev")]);
    git(&project.root, &["fetch", "-q", "origin"]);
    let delivered = project.run(&["deliver-merged", "--spec", SPEC]);
    assert_eq!(State::from_log(&project.log()).phase, Some("delivered"), "{delivered}");
    let overview = project.run(&["panel", "--spec", SPEC, "--section", "overview"]);
    let shown: Vec<(Value, Value)> = overview["deliveries"]["shown"]
        .as_array()
        .map(|rows| rows.iter().map(|row| (row["spec"].clone(), row["title"].clone())).collect())
        .unwrap_or_default();
    assert_eq!(shown, vec![(json!(SPEC), json!(title))], "the summary line carries the title: {overview}");
}

/// Um critério gravado sem declarar a forma dele é recusado, e a recusa lista
/// as cinco formas do padrão pelo nome, em vez de um nome de campo cru.
#[test]
fn criterion_without_a_declared_form_is_refused() {
    let project = Project::new();
    project.open();
    survey(&project);
    let said = user_says(&project, "O plano é uma onda só, que muda a saudação.");

    let refused = project.answer(&[
        "write",
        "criterion",
        "--spec",
        SPEC,
        "--json",
        &json!({"title": "Combinar o item", "when": "o programa roda", "then": "a saudação nova aparece", "proof": "git --version",
            "origin": said})
            .to_string(),
    ]);
    assert_eq!(refused["ok"], json!(false), "{refused}");
    assert_eq!(refused["reason"], json!("criterion-form-missing"), "{refused}");
    let hint = refused["hint"].as_str().unwrap_or_default();
    for shape in [
        "vale sempre",
        "disparada por um acontecimento",
        "estado durar",
        "recurso existir",
        "acontecimento indesejado",
    ] {
        assert!(hint.contains(shape), "a recusa lista a forma {shape:?} pelo nome: {hint}");
    }

    // Com a forma declarada, a mesma gravação passa.
    let accepted = project.write(
        "criterion",
        &json!({"title": "Combinar o item", "when": "o programa roda", "then": "a saudação nova aparece", "proof": "git --version",
            "form": "ubiquitous", "origin": said}),
    );
    assert_eq!(accepted["ok"], json!(true), "{accepted}");
}

/// A exigência da forma vale só para o critério que nasce agora, nunca para a
/// emenda de um critério antigo: um critério gravado direto no arquivo, sem
/// forma, como as specs de antes da exigência têm, recebe a emenda dele
/// também sem forma, e a gravação passa.
#[test]
fn amending_an_old_criterion_does_not_require_a_form() {
    let project = Project::new();
    project.open();
    survey(&project);
    let said = user_says(&project, "O plano é uma onda só, que muda a saudação.");

    // Um critério sem forma, escrito direto no arquivo, como as specs
    // antigas — de antes da exigência — têm.
    let path = store::spec_file(&project.root, SPEC).expect("spec file");
    let old_id = store::read(&path).expect("readable").expect("the spec file").max_id() + 1;
    let old_criterion = json!({"v": 1, "id": old_id, "code": "MSTD-CRIT-0001", "at": "2026-01-01T10:00:00-03:00",
        "type": "criterion", "author": "binary",
        "when": "o programa roda", "then": "a saudação antiga aparece", "proof": "git --version"});
    let mut content = std::fs::read_to_string(&path).expect("read the spec file");
    if !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(&old_criterion.to_string());
    content.push('\n');
    std::fs::write(&path, content).expect("write the old criterion");

    // A emenda dele, sem forma, passa: a exigência não vale para o critério
    // antigo.
    let amended = project.write(
        "criterion",
        &json!({"title": "Combinar o item", "when": "o programa roda", "then": "a saudação nova aparece", "proof": "git --version",
            "origin": said, "replaces": old_id}),
    );
    assert_eq!(amended["ok"], json!(true), "{amended}");

    // Um critério novo (sem `replaces`) continua exigindo a forma: a
    // exigência segue protegida para quem nasce agora.
    let new_criterion =
        json!({"title": "Outro efeito", "when": "outra coisa", "then": "outro efeito", "proof": "git --version", "origin": said});
    let refused_new =
        project.answer(&["write", "criterion", "--spec", SPEC, "--json", &new_criterion.to_string()]);
    assert_eq!(refused_new["ok"], json!(false), "{refused_new}");
    assert_eq!(refused_new["reason"], json!("criterion-form-missing"), "{refused_new}");
}

/// Os três termos internos usam o nome de mercado, nos dois idiomas: o que
/// era "combinado" vira "requisitos acordados", o que era "prova" vira
/// "verificação", e o que era "revisão final" vira "aceitação". O pedido de
/// verdade que o binário monta para o revisor é conferido no teste do fluxo de
/// ponta a ponta.
#[test]
fn three_terms_use_the_market_name() {
    let expected = [
        (Locale::PtBr, "page.block.agreed", "Requisitos acordados"),
        (Locale::EnUs, "page.block.agreed", "Agreed requirements"),
        (Locale::PtBr, "page.field.proof", "Verificação"),
        (Locale::EnUs, "page.field.proof", "Verification"),
        (Locale::PtBr, "page.field.final", "Aceitação"),
        (Locale::EnUs, "page.field.final", "Acceptance"),
        (Locale::PtBr, "prompt.part.agreed", "Requisitos acordados"),
        (Locale::EnUs, "prompt.part.agreed", "Agreed requirements"),
    ];
    for (locale, key, expected_text) in expected {
        assert_eq!(translate(key, locale), expected_text, "{key} ({locale:?}) usa o nome de mercado");
    }
}

/// A spec do projeto com submódulo, da abertura ao pull request: a onda muda
/// um arquivo do principal e um do submódulo, a rodada comita os dois e o
/// fechamento devolve a linha do `pr-open`, que roda. Confere o que a rodada
/// deixou nos dois repositórios e devolve a resposta do `pr-open`.
fn open_pull_requests_with_a_submodule(project: &Project) -> Value {
    let opened = project.open();
    assert_eq!(opened["step"], json!("ask_goal"), "{opened}");
    survey(project);
    plan_files(project, &["src/main.rs", SUB_FILE]);
    approve(project);

    // A cópia da onda traz o submódulo que a onda toca.
    let first = first_round(project);
    assert_eq!(first["dispatch"].as_array().map(Vec::len), Some(1), "{first}");
    let log = project.log();
    let sent = log.visible().into_iter().rfind(|e| e.event_type == "send").expect("the send");
    let copy = PathBuf::from(sent.str_field("copy").expect("the copy"));
    assert!(copy.join(SUB).join(".git").is_file(), "the copy brings the submodule the wave touches: {first}");

    std::fs::write(copy.join("src/main.rs"), "fn main() {\n    println!(\"olá\");\n}\n").expect("the change");
    std::fs::write(copy.join(SUB_FILE), "a biblioteca nova\n").expect("the submodule change");
    let delivered = json!({"wave": 1, "text": "A saudação e a biblioteca mudaram.",
        "files": ["src/main.rs", SUB_FILE], "commit": "a saudação e a biblioteca mudam",
        "agreed": agreed_all_met(project)});
    project.read_request(1);
    project.finish_tasks(1);
    project.run(&["write", "delivered", "--spec", SPEC, "--json", &delivered.to_string()]);
    let second = project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert!(
        copy.join(".git").is_file() && copy.join(SUB).join(".git").is_file(),
        "the copy and the submodule copy inside it stay for the next wave: {second}"
    );

    // O commit sai dentro do submódulo, na branch de mesmo nome, e o do
    // principal leva o ponteiro novo junto com o arquivo dele.
    let sub = project.root.join(SUB);
    assert_eq!(git_out(&sub, &["rev-parse", "--abbrev-ref", "HEAD"]), BRANCH, "{second}");
    assert_eq!(git_out(&project.root, &["rev-parse", "--abbrev-ref", "HEAD"]), BRANCH);
    assert_eq!(std::fs::read_to_string(sub.join("lib.txt")).unwrap(), "a biblioteca nova\n");
    assert_eq!(git_out(&sub, &["show", "-s", "--format=%s", "HEAD"]), "feat: a saudação e a biblioteca mudam");
    assert_eq!(git_out(&sub, &["show", "--name-only", "--format=", "HEAD"]), "lib.txt");
    assert_eq!(
        git_out(&project.root, &["rev-parse", &format!("HEAD:{SUB}")]),
        git_out(&sub, &["rev-parse", "HEAD"]),
        "the main commit carries the new pointer"
    );
    let changed = git_out(&project.root, &["show", "--name-only", "--format=", "HEAD"]);
    assert_eq!(changed.lines().collect::<Vec<_>>(), [SUB, "src/main.rs"], "{second}");
    assert_eq!(second["commit"]["submodules"][0]["path"], json!(SUB), "{second}");

    let asked = project.run(&["close", "--spec", SPEC]);
    assert_eq!(asked["review"]["final"], json!(true), "{asked}");
    let verdict = json!({"final": true, "result": "approved", "text": "Mudaram.", "agreed": agreed_all_met(project)});
    project.read_review();
    project.run(&["write", "verdict", "--spec", SPEC, "--json", &verdict.to_string()]);
    let closed = project.run(&["close", "--spec", SPEC]);
    assert!(!copy.exists(), "closing removes the copy of the work: {closed}");
    let inner = copy.join(SUB).to_string_lossy().replace('\\', "/");
    assert!(!git_out(&sub, &["worktree", "list", "--porcelain"]).contains(&inner), "git no longer lists the submodule copy: {closed}");
    let pr_line = closed["command"].as_str().expect("the pr-open line").to_string();
    let argv: Vec<&str> = pr_line.split_whitespace().skip(2).collect();
    project.run(&argv)
}

/// O evento de início de sessão, como a sessão o entrega.
fn session_start(project: &Project) -> Value {
    json!({"hook_event_name": "SessionStart", "source": "startup", "session_id": SESSION,
        "cwd": project.root.to_string_lossy()})
}

/// Duas conversas no mesmo projeto e na mesma branch: só a que abriu ou
/// retomou a spec grava nela. A que abriu grava sem `--spec` logo depois da
/// abertura. A outra não grava fala, resposta nem chamada, e a gravação dela
/// sem `--spec` é recusada com o comando de retomar. Depois do `/clear`, a
/// sessão nova retoma, e a fala e a gravação dela entram sem o aviso de outra
/// sessão. A leitura com `--root` de outro projeto não deixa chamada lá.
#[test]
fn only_the_conversation_bound_to_the_spec_records_in_it() {
    const PARALLEL: &str = "s-paralela";
    const CLEARED: &str = "s-ponta-depois-do-clear";
    let project = Project::new();
    let cwd = project.root.to_string_lossy().to_string();
    let says = |session: &str, text: &str| {
        project.hook("UserPromptSubmit", &json!({"hook_event_name": "UserPromptSubmit", "prompt": text, "session_id": session, "cwd": cwd}));
    };
    let message = |text: &str| {
        project.log().visible().into_iter().rfind(|e| e.event_type == "message" && e.str_field("text") == Some(text)).cloned()
    };
    let context = |text: &str, origin: u64| {
        json!({"title": "Combinar o item", "agent": "- conferir pelo teste", "text": text, "origin": origin}).to_string()
    };

    // A conversa que abre grava a fala e, sem `--spec`, o item.
    project.open();
    says(SESSION, GOAL);
    let goal = message(GOAL).expect("the conversation that opened records its message");
    assert_eq!(goal.str_field("session"), Some(SESSION), "the message says its conversation");
    let written = project.answer_as(SESSION, &["write", "context", "--json", &context(GOAL, goal.id)]);
    assert_eq!((&written["ok"], &written["spec"]), (&json!(true), &json!(SPEC)), "{written}");
    let item = project.log().visible().into_iter().rfind(|e| e.event_type == "context" && e.str_field("text") == Some(GOAL)).cloned();
    assert_eq!(item.expect("the item the conversation wrote").str_field("session"), Some(SESSION), "the item says its conversation");

    // A conversa paralela, sem ligação: nada dela entra.
    let before = (project.log().events.len(), project.operations().events.len());
    says(PARALLEL, "Na outra conversa, quero outra coisa.");
    project.hook(
        "Stop",
        &json!({"hook_event_name": "Stop", "last_assistant_message": "A resposta da outra conversa.", "session_id": PARALLEL, "cwd": cwd}),
    );
    let refused = project.answer_as(PARALLEL, &["write", "context", "--json", &context("O objetivo da outra conversa.", goal.id)]);
    assert_eq!(refused["reason"], json!("session-unbound"), "{refused}");
    assert!(refused["hint"].as_str().unwrap_or_default().contains("mustard-rt run resume"), "{refused}");
    project.run_as(PARALLEL, &["read", "conversation", "--spec", SPEC]);
    let after = (project.log().events.len(), project.operations().events.len());
    assert_eq!(after, before, "the parallel conversation left nothing in the spec");

    // O `/clear` troca a sessão; a nova retoma e grava sem o aviso.
    project.hook("SessionEnd", &json!({"hook_event_name": "SessionEnd", "reason": "clear", "session_id": SESSION, "cwd": cwd}));
    project.hook("SessionStart", &json!({"hook_event_name": "SessionStart", "source": "clear", "session_id": CLEARED, "cwd": cwd}));
    assert_eq!(project.run_as(CLEARED, &["resume"])["spec"], json!(SPEC));
    let later = "Depois de limpar, sigo na mesma obra.";
    says(CLEARED, later);
    let said = message(later).expect("the conversation that resumed records its message");
    assert_eq!(said.str_field("session"), Some(CLEARED));
    let again = project.run_as(CLEARED, &["write", "context", "--json", &context(later, said.id)]);
    assert!(!again.to_string().contains("another-session"), "{again}");

    // A leitura de outro projeto não grava chamada lá.
    let elsewhere = Project::new();
    elsewhere.run(&["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
    let there = elsewhere.root.to_string_lossy().to_string();
    project.run_as(CLEARED, &["read", "conversation", "--root", &there, "--spec", SPEC]);
    let reads = elsewhere.operations().visible().into_iter().filter(|e| e.str_field("command") == Some("read")).count();
    assert_eq!(reads, 0, "a reading from another conversation's project leaves no call there");
}

/// O agente da onda roda numa sessão sem ligação com a spec. De dentro da
/// cópia da onda, a leitura do pedido e a gravação do passo deixam na spec o
/// recibo da leitura e as duas chamadas, cada uma com o bloco pedido, mesmo
/// com as opções antes dele.
#[test]
fn the_wave_agent_calls_from_its_copy_count_without_a_binding() {
    const AGENT: &str = "s-agente-da-onda";
    let project = Project::new();
    project.open();
    survey(&project);
    plan(&project);
    approve(&project);
    first_round(&project);
    let log = project.log();
    let sent = log.visible().into_iter().rfind(|e| e.event_type == "send" && e.wave() == Some(1)).expect("the wave was sent");
    let copy = PathBuf::from(sent.str_field("copy").expect("the copy"));
    let task = log.visible().into_iter().find(|e| e.event_type == "task" && e.wave() == Some(1)).expect("the wave task").id;
    let task = log.codes().get(&task).cloned().expect("the task code");
    let root = project.root.display().to_string();

    let read = project.command_as(Some(AGENT), &copy, &["run", "read", "--root", &root, "--spec", SPEC, "request-1"], "");
    assert!(read.status.success(), "{}", String::from_utf8_lossy(&read.stdout));
    let step = json!({"wave": 1, "item": task, "text": "A tarefa ficou pronta."}).to_string();
    let written = project.command_as(Some(AGENT), &copy, &["run", "write", "step", "--root", &root, "--spec", SPEC, "--json", &step], "");
    assert!(written.status.success(), "{}", String::from_utf8_lossy(&written.stdout));

    let receipts = project.log().visible().into_iter().filter(|e| e.event_type == "call" && e.str_field("request") == Some("request-1")).count();
    assert!(receipts > 0, "the reading of the request left its receipt");
    let operations = project.operations();
    let invoked: Vec<(&str, &str)> = operations
        .visible()
        .into_iter()
        .filter(|e| e.event_type == "call" && e.fields.get("invocation") == Some(&json!(true)))
        .filter_map(|e| Some((e.str_field("command")?, e.str_field("block")?)))
        .collect();
    assert!(invoked.contains(&("read", "request-1")) && invoked.contains(&("write", "step")), "{invoked:?}");
}

/// A conversa que abriu a spec na raiz do projeto segue gravando nela de uma
/// subpasta: a fala e a resposta que o gancho recebe com o `cwd` ali, rodado
/// dali e sem a pasta do projeto no ambiente, e a busca rodada dali. A
/// sessão cujo nome sairia da pasta das sessões não liga nada: a retomada
/// por ela não grava arquivo fora dali.
#[test]
fn the_conversation_opened_at_the_root_records_from_a_subfolder() {
    let project = Project::new();
    project.open();
    let sub = project.root.join("src");
    let cwd = sub.to_string_lossy().to_string();

    let said = "Da subpasta, a fala segue na obra.";
    project.hook_from(&sub, "UserPromptSubmit", &json!({"hook_event_name": "UserPromptSubmit", "prompt": said, "session_id": SESSION, "cwd": cwd}));
    let replied = "A resposta dada na subpasta.";
    project.hook_from(&sub, "Stop", &json!({"hook_event_name": "Stop", "last_assistant_message": replied, "session_id": SESSION, "cwd": cwd}));
    let log = project.log();
    let recorded = |kind: &str, text: &str| log.visible().into_iter().any(|e| e.event_type == kind && e.str_field("text") == Some(text));
    assert!(recorded("message", said) && recorded("response", replied), "the message and the reply from the subfolder enter the spec");

    let search = ["run", "search", "--shell-output", "--intent", "Find the greeting", "--purpose", "locate", "--", "grep", "-n", "println", "main.rs"];
    let out = project.command_as(Some(SESSION), &sub, &search, "");
    assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let searches = project.operations().visible().into_iter().filter(|e| e.event_type == "call" && e.str_field("command") == Some("search")).count();
    assert_eq!(searches, 1, "the search run from the subfolder counts as a call of the conversation");

    let escaping = "../../fora";
    let resumed = project.command_as(Some(escaping), &project.root, &["run", "resume"], "");
    assert!(resumed.status.success(), "{}", String::from_utf8_lossy(&resumed.stdout));
    assert!(!project.root.join("fora").exists(), "a session named outside the sessions folder writes nothing out there");
}

/// O `_meta` da chamada que o Codex manda a um servidor MCP: a captura do
/// `codex` 0.160.1 (`fixtures/codex-tools-call.json`), com os ids da conversa
/// trocados por `session`.
fn codex_meta(session: &str) -> Value {
    let call: Value = serde_json::from_str(&include_str!("fixtures/codex-tools-call.json").replace("01a16614-52a7-73e3-a596-56c652948934", session)).unwrap();
    call["params"]["_meta"].clone()
}

/// Uma busca pelo servidor `mustard-rt mcp` do projeto, rodado sem pasta de
/// sessão no ambiente a menos que `env_session` a dê: `initialize` e depois o
/// `tools/call`, com `meta` no `_meta` da chamada quando há. Devolve a
/// resposta da chamada.
fn mcp_search(project: &Project, env_session: Option<&str>, meta: Option<Value>) -> Value {
    let request = json!({"tool": "Grep", "input": {"pattern": "println", "path": project.root.join("src"), "output_mode": "content"},
        "intent": "Find the greeting", "purpose": "locate"});
    let mut params = json!({"name": "search", "arguments": {"request": request}});
    if let Some(meta) = meta {
        params["_meta"] = meta;
    }
    let lines = [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-11-25"}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": params}),
    ]
    .map(|line| line.to_string() + "\n")
    .concat();
    let root = project.root.to_string_lossy().to_string();
    let out = project.command_as(env_session, &project.root, &["mcp", "--root", &root], &lines);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let replies: Vec<Value> = String::from_utf8_lossy(&out.stdout).lines().filter_map(|line| serde_json::from_str(line).ok()).collect();
    let reply = replies.into_iter().find(|reply| reply["id"] == json!(2)).expect("the server answers the call");
    assert_eq!(reply["result"]["isError"], json!(false), "{reply}");
    reply
}

/// A busca que a conversa faz pelo servidor `mustard-rt mcp` conta nas
/// operações da spec a que a sessão dela está ligada, venha a sessão pelo
/// `_meta` que o Codex manda, só pelo turno dele ou pelo ambiente de quem subiu
/// o servidor. O `_meta` vale mais que o ambiente, que fica velho
/// depois do `/clear`; a chamada sem sessão, ou de sessão sem ligação, não
/// entra na spec de ninguém.
#[test]
fn the_mcp_search_server_counts_its_calls_in_the_spec_of_the_conversation() {
    let project = Project::new();
    project.open();
    let searches = || -> Vec<SpecEvent> {
        project.operations().visible().into_iter().filter(|e| e.event_type == "call" && e.str_field("command") == Some("search")).cloned().collect()
    };
    assert!(searches().is_empty(), "a spec recém-aberta não tem busca");

    mcp_search(&project, None, Some(codex_meta(SESSION)));
    let counted = searches();
    assert_eq!(counted.len(), 1, "a sessão dita no _meta que o Codex manda conta na spec");
    assert_eq!(counted[0].str_field("session"), Some(SESSION), "a chamada leva a sessão que a gravou");

    mcp_search(&project, None, Some(json!({"x-codex-turn-metadata": {"session_id": "s-raiz", "thread_id": SESSION, "turn_id": "t1"}})));
    assert_eq!(searches().len(), 2, "o turno que o Codex manda no _meta também liga a chamada à conversa");

    mcp_search(&project, None, None);
    assert_eq!(searches().len(), 2, "sem sessão nenhuma, a chamada não entra na spec");

    mcp_search(&project, Some(SESSION), None);
    assert_eq!(searches().len(), 3, "a sessão do ambiente do servidor segue valendo");

    mcp_search(&project, Some(SESSION), Some(codex_meta("s-outra")));
    assert_eq!(searches().len(), 3, "o _meta vale mais que o ambiente: a sessão sem ligação não grava na spec da outra");
}

/// A fala que o usuário diz com um ponto do levantamento aberto entra pelo
/// gancho com o número do ponto, e a leitura da conversa pelo comando a
/// mostra apontando o ponto pelo código dele.
#[test]
fn a_message_said_during_an_open_point_reads_with_the_point_code() {
    let project = Project::new();
    project.open();
    let (_, _, open) = open_the_points(&project, "Combinar o item", GOAL);
    let code = open["code"].as_str().unwrap_or_else(|| panic!("an open point: {open}")).to_string();
    let question = "A saudação muda em todo lugar?";
    let asked = user_says(&project, question);
    assert_eq!(project.log().get(asked).and_then(|message| message.int("during")), open["id"].as_u64(), "the file keeps the point number");

    let read = project.run(&["read", "conversation", "--spec", SPEC]);

    let shown = read["events"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|line| line["text"] == json!(question))
        .unwrap_or_else(|| panic!("the message is read: {read}"));
    assert_eq!(shown["during"], json!(code), "the open point is shown by its code: {shown}");
}

/// A marca da ligação da conversa `session` com a spec, onde a abertura a
/// grava: na raiz do projeto, e não na pasta de onde a conversa roda.
fn binding_mark(project: &Project, session: &str) -> PathBuf {
    project.root.join(".claude/.session").join(session).join("active-spec")
}

/// O fechamento rodado de uma subpasta, pela conversa que abriu a spec na
/// raiz, apaga a ligação que a abertura gravou lá: a conversa seguinte não
/// grava mais na spec fechada.
#[test]
fn closing_from_a_subfolder_clears_the_binding_the_open_wrote_at_the_root() {
    let project = Project::new();
    let sub = project.root.join("src");
    let mark = binding_mark(&project, SESSION);

    let line = closed_by(&project, "Combinar o item", GOAL, &|args| {
        assert!(mark.exists(), "the conversation stays bound until the spec closes: {}", mark.display());
        let report = project.answer_in(Some(SESSION), &sub, args);
        assert_eq!(report["ok"], json!(true), "{args:?}: {report}");
        report
    });

    assert!(!line.is_empty(), "the close still names the pull request line");
    assert!(!mark.exists(), "the closed spec leaves no binding mark at the root: {}", mark.display());
}

/// O descarte rodado de uma subpasta, pela conversa que abriu a spec na raiz,
/// apaga a ligação que a abertura gravou lá.
#[test]
fn discarding_from_a_subfolder_clears_the_binding_the_open_wrote_at_the_root() {
    let project = Project::new();
    let sub = project.root.join("src");
    let mark = binding_mark(&project, SESSION);
    project.open();
    assert!(mark.exists(), "the open binds the conversation at the root: {}", mark.display());

    let preview = project.answer_in(Some(SESSION), &sub, &["discard", "--spec", SPEC]);
    let token = preview["token"].as_str().expect("the discard code").to_string();
    assert!(mark.exists(), "the preview discards nothing");
    let done = project.answer_in(Some(SESSION), &sub, &["discard", "--spec", SPEC, "--confirm", &token]);

    assert_eq!(done["ok"], json!(true), "{done}");
    assert!(!mark.exists(), "the discarded spec leaves no binding mark at the root: {}", mark.display());
}

/// Outra pessoa mergeia o pull request do submódulo, pelo `gh` de fora do
/// binário.
fn someone_merges_the_submodule(project: &Project) {
    let merged = project
        .env(&mut Command::new(project.bin.join("gh")))
        .args(["pr", "merge", "3", "--merge"])
        .current_dir(project.root.join(SUB))
        .output();
    assert!(merged.expect("the fake gh").status.success(), "someone else merges the submodule pull request");
}

/// A ponta da base do submódulo no servidor dele, que é o ponteiro que o
/// principal passa a gravar depois do merge.
fn submodule_base_tip(project: &Project) -> String {
    git_out(&project.remotes.join("sub.git"), &["rev-parse", "refs/heads/main"])
}

/// O ponteiro do submódulo gravado no commit do principal.
fn pointer(project: &Project) -> String {
    git_out(&project.root, &["rev-parse", &format!("HEAD:{SUB}")])
}

/// O ponteiro que o principal tinha quando o pull request dele foi marcado
/// como pronto, anotado pelo `gh` falso.
fn pointer_when_ready(project: &Project) -> String {
    std::fs::read_to_string(project.remotes.join("ready-main"))
        .expect("the main pull request was marked ready")
        .trim()
        .to_string()
}

/// Quando uma spec mexe no principal e num submódulo, as duas branches têm o
/// mesmo nome, o pull request do submódulo abre primeiro, contra a base dele,
/// e o do principal abre como rascunho. Enquanto o do submódulo não entra, o
/// início da sessão diz qual falta e o principal segue rascunho; quando ele
/// entra pelo `pr-merge`, o principal recebe o ponteiro da base do submódulo,
/// envia a branch e fica pronto.
#[test]
fn a_spec_on_the_main_repository_and_a_submodule_readies_the_main_pull_request_only_after_the_submodule_one() {
    let project = Project::with_submodule();
    let pr = open_pull_requests_with_a_submodule(&project);

    // O do submódulo abre primeiro, da branch de mesmo nome contra a base
    // dele; o do principal, depois, como rascunho.
    let calls = project.gh_calls();
    let created = |repo: &str| {
        calls
            .iter()
            .position(|call| call.starts_with(&format!("{repo} pr create")))
            .unwrap_or_else(|| panic!("no pull request opened in {repo}: {calls:#?}"))
    };
    let (sub_at, main_at) = (created("sub"), created("main"));
    assert!(sub_at < main_at, "the submodule pull request opens first: {calls:#?}");
    assert!(calls[sub_at].contains(&format!("--head {BRANCH} --base main")), "{}", calls[sub_at]);
    assert!(!calls[sub_at].contains("--draft"), "{}", calls[sub_at]);
    assert!(calls[main_at].contains(&format!("--head {BRANCH} --base dev")), "{}", calls[main_at]);
    assert!(calls[main_at].contains("--draft"), "the main pull request opens as a draft: {}", calls[main_at]);
    let sub_server = project.remotes.join("sub.git");
    assert!(
        !git_out(&sub_server, &["for-each-ref", &format!("refs/heads/{BRANCH}")]).is_empty(),
        "the submodule branch, with the same name, is on its server"
    );
    assert!(
        !git_out(&project.remotes.join("projeto.git"), &["for-each-ref", &format!("refs/heads/{BRANCH}")]).is_empty(),
        "the main branch is on its server once the pull request is open"
    );
    assert_eq!(pr["number"], json!(7), "{pr}");
    assert_eq!(pr["submodules"][0]["path"], json!(SUB), "{pr}");
    assert_eq!(pr["submodules"][0]["url"], json!("https://github.com/exemplo/sub/pull/3"), "{pr}");
    let waiting = translate("pr.submodules.waiting", Locale::PtBr).replace("{pr}", "7").replace("{paths}", SUB);
    assert_eq!(pr["hint"], json!(waiting), "{pr}");

    // Enquanto o do submódulo não entra, o início da sessão diz qual falta, e
    // o principal segue rascunho.
    let said = project.hook("SessionStart", &session_start(&project));
    assert!(said.contains(&waiting), "the session start names the missing pull request: {said}");
    assert!(!project.gh_calls().iter().any(|call| call.starts_with("main pr ready")), "still a draft");

    // O do submódulo entra pelo `pr-merge`: o ponteiro vai para a base do
    // submódulo, a branch do principal é enviada, e o principal fica pronto.
    let sub = project.root.join(SUB);
    let before = pointer(&project);
    let merged = project.run(&["pr-merge", "--pr", "3", "--root", &sub.to_string_lossy()]);
    assert_eq!(merged["action"], json!("merged"), "{merged}");
    assert_eq!(merged["submodules"]["ready"], json!(true), "{merged}");
    let sub_base = submodule_base_tip(&project);
    let pointer = pointer(&project);
    assert_ne!(pointer, before, "the pointer moved: {merged}");
    assert_eq!(pointer, sub_base, "the pointer is the submodule base after the merge: {merged}");
    assert_eq!(pointer_when_ready(&project), sub_base, "the pointer was already in the main when it went ready");
    assert_eq!(
        git_out(&project.remotes.join("projeto.git"), &["rev-parse", &format!("refs/heads/{BRANCH}")]),
        git_out(&project.root, &["rev-parse", "HEAD"]),
        "the main branch was pushed with the pointer"
    );
    let calls = project.gh_calls();
    let merged_at = calls.iter().position(|call| call.starts_with("sub pr merge 3")).expect("the submodule merge");
    let ready_at = calls.iter().position(|call| call.starts_with("main pr ready 7")).expect("the main pull request is ready");
    assert!(merged_at < ready_at, "{calls:#?}");
    assert!(git_out(&sub, &["branch", "--list", BRANCH]).is_empty(), "the submodule branch left this machine");
}

/// O pull request do submódulo que outra pessoa mergeou é achado no início da
/// sessão: o ponteiro vai para a base do submódulo, a branch do principal é
/// enviada, o principal fica pronto e o aviso diz isso.
#[test]
fn a_submodule_pull_request_merged_by_someone_else_readies_the_main_one_at_session_start() {
    let project = Project::with_submodule();
    open_pull_requests_with_a_submodule(&project);

    someone_merges_the_submodule(&project);
    assert!(!project.gh_calls().iter().any(|call| call.starts_with("main pr ready")), "still a draft");

    let start = session_start(&project);
    let said = project.hook("SessionStart", &start);
    let ready = translate("pr.submodules.ready", Locale::PtBr).replace("{pr}", "7").replace("{paths}", SUB);
    assert!(said.contains(&ready), "the session start says the main pull request is ready: {said}");
    let sub_base = submodule_base_tip(&project);
    assert_eq!(pointer(&project), sub_base, "the pointer moved");
    assert_eq!(
        git_out(&project.remotes.join("projeto.git"), &["rev-parse", &format!("refs/heads/{BRANCH}")]),
        git_out(&project.root, &["rev-parse", "HEAD"]),
        "the main branch was pushed with the pointer"
    );
    assert!(project.gh_calls().iter().any(|call| call.starts_with("main pr ready 7")), "the main pull request is ready");
    assert_eq!(pointer_when_ready(&project), sub_base, "the pointer was already in the main when it went ready");

    // A conferência seguinte não refaz nada: o principal já está pronto.
    let commits = git_out(&project.root, &["rev-list", "--count", "HEAD"]);
    let again = project.hook("SessionStart", &start);
    assert!(!again.contains(&ready), "{again}");
    assert_eq!(git_out(&project.root, &["rev-list", "--count", "HEAD"]), commits, "no second pointer commit");
}

/// O submódulo que entrou e já não tem a branch na máquina ainda leva o
/// ponteiro ao principal: o pronto vem depois do ponteiro, nunca sem ele. A
/// branch na máquina não diz nada sobre o ponteiro — quem decide é o fato,
/// depois de buscar a base.
#[test]
fn a_landed_submodule_without_its_branch_here_still_moves_the_pointer_before_the_ready() {
    let project = Project::with_submodule();
    open_pull_requests_with_a_submodule(&project);
    someone_merges_the_submodule(&project);

    // A branch sai da máquina antes da conferência.
    let sub = project.root.join(SUB);
    git(&sub, &["checkout", "-q", "--detach", BRANCH]);
    git(&sub, &["branch", "-qD", BRANCH]);

    project.hook("SessionStart", &session_start(&project));
    let sub_base = submodule_base_tip(&project);
    assert_eq!(pointer(&project), sub_base, "the pointer moved although the branch had left");
    assert_eq!(pointer_when_ready(&project), sub_base, "the pointer was already in the main when it went ready");
}

/// Um commit do submódulo que o merge não levou trava a conferência: nada é
/// mexido, a branch fica com ele, o principal segue rascunho e a resposta diz
/// o motivo.
#[test]
fn a_submodule_commit_the_merge_did_not_take_keeps_the_branch_and_the_draft() {
    let project = Project::with_submodule();
    open_pull_requests_with_a_submodule(&project);

    // O merge leva o que está no servidor do submódulo; este commit fica só
    // aqui, e nunca foi enviado.
    let sub = project.root.join(SUB);
    std::fs::write(sub.join("lib.txt"), "a biblioteca ainda mais nova\n").expect("the change");
    git(&sub, &["commit", "-qam", "o conserto que ficou aqui"]);
    let kept = git_out(&sub, &["rev-parse", "HEAD"]);

    let before = pointer(&project);
    let merged = project.answer(&["pr-merge", "--pr", "3", "--root", &sub.to_string_lossy()]);
    assert_eq!(merged["submodules"]["ready"], json!(false), "{merged}");
    let problem = merged["submodules"]["problem"].as_str().unwrap_or_default().to_string();
    assert!(problem.contains(SUB) && problem.contains(BRANCH), "the refusal names the branch that is ahead: {merged}");
    assert_eq!(pointer(&project), before, "the pointer did not move: {merged}");
    assert_eq!(git_out(&sub, &["rev-parse", BRANCH]), kept, "the local commit still has a branch: {merged}");
    assert!(!project.gh_calls().iter().any(|call| call.starts_with("main pr ready")), "still a draft");
}

/// O `pr-open` que roda de novo com o pull request do submódulo já mergeado
/// não o dá por pronto quando sobra commit aqui: para com o motivo, antes de
/// mexer no pull request do principal.
#[test]
fn a_second_pr_open_refuses_a_merged_submodule_that_still_carries_work() {
    let project = Project::with_submodule();
    open_pull_requests_with_a_submodule(&project);
    someone_merges_the_submodule(&project);

    let sub = project.root.join(SUB);
    std::fs::write(sub.join("lib.txt"), "a biblioteca ainda mais nova\n").expect("the change");
    git(&sub, &["commit", "-qam", "o conserto que ficou aqui"]);

    let opened = project.answer(&["pr-open", "--base", "dev", "--head", BRANCH, "--spec", SPEC]);
    assert_eq!(opened["ok"], json!(false), "{opened}");
    let error = opened["error"].as_str().unwrap_or_default().to_string();
    assert!(error.contains(SUB) && error.contains(BRANCH), "the refusal names the branch that is ahead: {opened}");
    assert_eq!(opened["submodules"][0]["action"], json!("open"), "the submodule is not reported as done: {opened}");
}

/// O envio do ponteiro que o servidor recusa deixa o principal como rascunho,
/// com o motivo e a branch do submódulo no lugar; a conferência seguinte, com
/// o servidor aceitando, refaz o envio e só então marca o principal pronto.
#[test]
fn a_submodule_pointer_the_server_refuses_keeps_the_main_pull_request_a_draft() {
    let project = Project::with_submodule();
    open_pull_requests_with_a_submodule(&project);

    let refuse = project.remotes.join("projeto.git/hooks/pre-receive");
    executable::write_executable(&refuse, "#!/bin/sh\nexit 1\n");

    let sub = project.root.join(SUB);
    let merged = project.answer(&["pr-merge", "--pr", "3", "--root", &sub.to_string_lossy()]);
    assert_eq!(merged["submodules"]["ready"], json!(false), "{merged}");
    assert!(merged["submodules"]["problem"].as_str().is_some_and(|r| r.starts_with("push")), "{merged}");
    assert!(!project.gh_calls().iter().any(|call| call.starts_with("main pr ready")), "still a draft");
    assert!(!git_out(&sub, &["branch", "--list", BRANCH]).is_empty(), "the submodule branch is still here");

    // O servidor volta a aceitar: a conferência seguinte refaz o envio.
    std::fs::remove_file(&refuse).expect("the server hook leaves");
    project.hook("SessionStart", &session_start(&project));
    assert_eq!(
        git_out(&project.remotes.join("projeto.git"), &["rev-parse", &format!("refs/heads/{BRANCH}")]),
        git_out(&project.root, &["rev-parse", "HEAD"]),
        "the main branch was pushed with the pointer"
    );
    assert_eq!(pointer_when_ready(&project), submodule_base_tip(&project), "the pointer came before the ready");
}

/// Duas sessões conferindo ao mesmo tempo: a trava do passo do git faz uma
/// esperar a outra, o ponteiro entra uma vez só e nenhuma delas volta com o
/// erro cru do git.
#[test]
fn two_sessions_checking_the_submodule_at_the_same_time_move_the_pointer_once() {
    let project = Project::with_submodule();
    open_pull_requests_with_a_submodule(&project);
    someone_merges_the_submodule(&project);

    // O commit do ponteiro demora, e as duas sessões se encontram dentro dele.
    let hook = project.root.join(".git/hooks/pre-commit");
    executable::write_executable(&hook, "#!/bin/sh\nsleep 1\n");

    let commits = git_out(&project.root, &["rev-list", "--count", "HEAD"]);
    let start = session_start(&project);
    let (one, two) = std::thread::scope(|scope| {
        let first = scope.spawn(|| project.hook("SessionStart", &start));
        let second = scope.spawn(|| project.hook("SessionStart", &start));
        (first.join().expect("a session"), second.join().expect("the other session"))
    });

    let stuck = translate("pr.submodules.stuck", Locale::PtBr).replace("{pr}", "7");
    let stuck = stuck.split("{reason}").next().unwrap_or_default().to_string();
    for said in [&one, &two] {
        assert!(!said.contains(&stuck), "neither session is stuck on the other's git step: {said}");
    }
    let after: usize = git_out(&project.root, &["rev-list", "--count", "HEAD"]).parse().expect("a number");
    assert_eq!(after, commits.parse::<usize>().expect("a number") + 1, "the pointer commit happened once");
    assert_eq!(pointer(&project), submodule_base_tip(&project), "the pointer is the submodule base");
}

/// A abertura da spec deixa o mapa do projeto em dia e, com ele gravado,
/// começa em segundo plano a leitura da história de todo arquivo dele: uma
/// passada e uma leitura, as duas sobre o mapa do projeto.
#[test]
fn opening_a_spec_starts_the_reading_of_the_history_of_the_map_it_refreshes() {
    let project = Project::with_scan();
    let opened = project.open();
    assert_eq!(opened["step"], json!("ask_goal"), "{opened}");

    let model = map_store::model_path(&project.root);
    assert_eq!(
        project.scan_passes(),
        [format!("scan {} --out {} --json", project.root.display(), model.display())],
        "the open asks one pass over the project, for its map"
    );
    assert_eq!(
        project.history_reads_after_waiting(1),
        [project.model_read_line()],
        "the open that refreshed the map starts the reading of the history of every file, in the background"
    );
}

/// A pasta temporária que tem dois nomes — o atalho e o caminho de verdade,
/// como `/var` e `/private/var` no Mac — não muda o que a abertura pede ao
/// scan: o projeto e o mapa dele saem pelo caminho de verdade nas duas
/// pontas, a que o programa anota e a que o teste espera.
#[test]
fn a_project_in_a_temp_folder_with_two_names_is_asked_about_by_its_real_path() {
    let real = tempfile::tempdir().expect("the real folder");
    let aside = tempfile::tempdir().expect("the folder of the shortcut");
    let shortcut = aside.path().join("atalho");
    std::os::unix::fs::symlink(real.path(), &shortcut).expect("the shortcut");

    let project = Project::with_scan_in(tempfile::tempdir_in(&shortcut).expect("a folder through the shortcut"));
    assert!(
        project.root.starts_with(real.path().canonicalize().expect("the real path")),
        "the project is named by the real path, not through the shortcut: {}",
        project.root.display()
    );
    let opened = project.open();
    assert_eq!(opened["step"], json!("ask_goal"), "{opened}");
    let model = map_store::model_path(&project.root);
    assert_eq!(
        project.scan_passes(),
        [format!("scan {} --out {} --json", project.root.display(), model.display())],
        "the open asks the pass over the project by its real path"
    );
}

/// A spec no ponto da entrega da onda 1: a cópia mudada e a entrega gravada
/// pelo agente da onda, ainda por assumir.
fn deliver_the_first_wave(project: &Project) {
    deliver_the_first_wave_changing(project, "src/main.rs");
}

/// [`deliver_the_first_wave`] com a onda mudando o arquivo `file`.
fn deliver_the_first_wave_changing(project: &Project, file: &str) {
    project.open();
    survey(project);
    plan_files(project, &[file]);
    approve(project);
    first_round(project);
    let log = project.log();
    let sent = log.visible().into_iter().rfind(|e| e.event_type == "send").expect("the send");
    let copy = PathBuf::from(sent.str_field("copy").expect("the copy"));
    std::fs::write(copy.join(file), "fn main() {\n    println!(\"olá\");\n}\n").expect("the change");
    let delivered = json!({"wave": 1, "text": "A saudação virou olá.", "files": [file],
        "commit": "a saudação vira olá", "agreed": agreed_all_met(project)});
    project.read_request(1);
    project.finish_tasks(1);
    project.run(&["write", "delivered", "--spec", SPEC, "--json", &delivered.to_string()]);
}

/// As passadas e as leituras da história que uma chamada trouxe, contadas
/// desde o ponto `(passes, reads)` anterior a ela; a lista de leituras só
/// vale depois de o processo em segundo plano parar de anotar. A primeira
/// leitura nova é esperada antes, por um tempo: na máquina carregada, o
/// processo em segundo plano demora mais que a pausa da conferência a começar.
fn since(project: &Project, before: &(usize, usize)) -> (Vec<String>, Vec<String>) {
    project.history_reads_after_waiting(before.1 + 1);
    let reads = project.settled_history_reads();
    (project.scan_passes()[before.0..].to_vec(), reads[before.1..].to_vec())
}

/// A passada que leu uma cópia descartável do mapa, e não o mapa do projeto:
/// a que a conferência depois da onda pede e joga fora.
fn is_throwaway_pass(project: &Project, pass: &str) -> bool {
    let model = map_store::model_path(&project.root);
    pass.starts_with("scan ") && !pass.contains(&format!("--out {} ", model.display()))
}

/// A rodada que assume a entrega e comita relê o mapa do projeto e começa a
/// leitura da história dele; a cópia do mapa que a conferência depois da onda
/// relê e joga fora só ganha a passada.
#[test]
fn a_round_that_commits_the_return_starts_the_reading_of_the_history_of_the_project_map_only() {
    let project = Project::with_scan();
    deliver_the_first_wave(&project);
    let before = (project.scan_passes().len(), project.settled_history_reads().len());

    let second = project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert!(second.get("commit").is_some(), "the round committed the return: {second}");

    let (passes, reads) = since(&project, &before);
    assert!(passes.iter().any(|pass| is_throwaway_pass(&project, pass)), "the check after the wave read a copy of the map: {passes:?}");
    assert!(!reads.is_empty(), "the round that refreshed the map starts the reading of its history: {passes:?}");
    assert!(
        reads.iter().all(|read| *read == project.model_read_line()),
        "the reading is over the map of the project, never over the copy that is thrown away: {reads:?}"
    );
}

/// O fechamento que assume a última entrega, pela mesma porta da rodada,
/// começa a leitura da história do mapa do projeto que ele releu.
#[test]
fn a_close_that_takes_the_last_return_starts_the_reading_of_the_history_of_the_project_map_only() {
    let project = Project::with_scan();
    deliver_the_first_wave(&project);
    let before = (project.scan_passes().len(), project.settled_history_reads().len());

    let asked = project.run(&["close", "--spec", SPEC]);
    assert_eq!(asked["phase"], json!("running"), "{asked}");

    let (passes, reads) = since(&project, &before);
    assert!(passes.iter().any(|pass| is_throwaway_pass(&project, pass)), "the check after the wave read a copy of the map: {passes:?}");
    assert!(!reads.is_empty(), "the close that refreshed the map starts the reading of its history: {passes:?}");
    assert!(
        reads.iter().all(|read| *read == project.model_read_line()),
        "the reading is over the map of the project, never over the copy that is thrown away: {reads:?}"
    );
}

/// O `cargo` de mentira de quem compila a versão em construção do Mustard
/// recebe a chamada de cada compilação como `<pasta> <argumentos>`.
fn the_build_call(project: &Project) -> String {
    format!(
        "{} build --profile mustard-dev --locked -p mustard-rt -p scan -p mustard-cli --target-dir {}",
        project.root.display(),
        project.build_folder().display()
    )
}

/// A rodada que levou ao commit uma onda que mexeu no programa do Mustard
/// compila a versão em construção: uma vez, no checkout principal, com os três
/// programas, na pasta de compilação do projeto, e sem recusar nem avisar
/// quando a compilação passa.
#[test]
fn a_round_that_committed_the_program_builds_the_development_version_once_in_the_main_checkout() {
    let project = Project::as_mustard_source();
    deliver_the_first_wave_changing(&project, "apps/rt/src/main.rs");
    assert!(project.cargo_calls().is_empty(), "nada compila antes da rodada: {:?}", project.cargo_calls());

    let second = project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert!(second.get("commit").is_some(), "the round committed the return: {second}");
    assert_eq!(project.cargo_calls(), [changed_build_call(&project)], "{second}");
    let reasons: Vec<&str> = second["warnings"].as_array().into_iter().flatten().filter_map(|w| w["reason"].as_str()).collect();
    assert!(!reasons.contains(&"development-build-failed"), "a compilação verde não avisa: {second}");
}

/// A onda que não tocou `apps/` nem `packages/` comita sem compilar nada,
/// mesmo no repositório do Mustard.
#[test]
fn a_round_whose_commit_left_the_program_alone_does_not_build() {
    let project = Project::as_mustard_source();
    deliver_the_first_wave_changing(&project, "src/main.rs");

    let second = project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert!(second.get("commit").is_some(), "the round committed the return: {second}");
    assert!(project.cargo_calls().is_empty(), "o commit não tocou o programa: {:?}", project.cargo_calls());
}

/// Fora do repositório do Mustard a rodada nunca compila, nem com um arquivo
/// sob `apps/` no commit.
#[test]
fn a_round_outside_the_mustard_repository_never_builds() {
    let project = Project::as_mustard_source();
    std::fs::remove_file(project.root.join("apps/rt/Cargo.toml")).expect("the manifest goes");
    git(&project.root, &["commit", "-q", "-a", "-m", "o projeto deixa de ser o Mustard"]);
    deliver_the_first_wave_changing(&project, "apps/rt/src/main.rs");

    let second = project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert!(second.get("commit").is_some(), "the round committed the return: {second}");
    assert!(project.cargo_calls().is_empty(), "outro projeto: {:?}", project.cargo_calls());
}

/// A compilação vermelha vira um aviso com o fim da saída do `cargo`, e o
/// commit sai do mesmo jeito: a rodada nunca recusa por causa dela.
#[test]
fn a_red_development_build_warns_with_the_output_and_the_round_still_commits() {
    let project = Project::as_mustard_source();
    deliver_the_first_wave_changing(&project, "apps/rt/src/main.rs");
    std::fs::write(project.home.join("cargo-falha"), "").expect("the build is red");

    let second = project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert!(second.get("commit").is_some(), "the round committed anyway: {second}");
    assert_eq!(project.cargo_calls(), [changed_build_call(&project)], "{second}");
    let warning = second["warnings"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|w| w["reason"] == json!("development-build-failed"))
        .cloned()
        .unwrap_or_else(|| panic!("o aviso da compilação vermelha: {second}"));
    let hint = warning["hint"].as_str().unwrap_or_default();
    let head = translate("round.development_build_failed", Locale::PtBr);
    assert!(hint.starts_with(head.split("{output}").next().unwrap_or_default()), "{warning}");
    assert!(hint.contains("error[E0425]: cannot find value `x`"), "{warning}");
}

/// O texto que o início da sessão põe no contexto, ou nada.
fn session_notices(project: &Project) -> String {
    let said = project.hook("SessionStart", &session_start(project));
    serde_json::from_str::<Value>(&said)
        .ok()
        .and_then(|out| out["hookSpecificOutput"]["additionalContext"].as_str().map(str::to_string))
        .unwrap_or_default()
}

/// O aviso do compilado de `key`, com o commit atual do projeto e o que
/// aconteceu com a compilação em segundo plano, até o ponto em que o programa
/// que roda a sessão é dito: cortado antes da versão dele, que muda a cada
/// compilação.
fn build_notice_head(project: &Project, key: &str, compiled: &str, progress: &str) -> String {
    let text = translate(key, Locale::PtBr)
        .replace("{head}", &project.head())
        .replace("{compiled}", compiled)
        .replace("{progress}", translate(progress, Locale::PtBr));
    text.split("{running}").next().unwrap_or_default().to_string()
}

/// Espera a compilação em segundo plano acabar: a trava dela passa a dizer
/// como terminou (`built` ou `failed`).
fn wait_for_the_background_build(project: &Project) {
    for _ in 0..200 {
        if std::fs::read_to_string(project.build_marker()).is_ok_and(|said| !said.trim().is_empty()) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("a compilação em segundo plano nunca escreveu na trava como terminou");
}

/// No repositório do Mustard sem o programa compilado, o início da sessão
/// solta a compilação em segundo plano, uma vez e no checkout principal, e
/// avisa que a sessão roda o programa instalado até ela acabar.
#[test]
fn a_session_in_the_mustard_repository_without_the_compiled_program_starts_the_build_and_says_so() {
    let project = Project::as_mustard_source();

    let said = session_notices(&project);
    let expected = build_notice_head(&project, "session.build.missing", "", "session.build.started");
    assert!(said.contains(&format!("{expected}{}", env!("CARGO_PKG_VERSION"))), "o aviso diz o programa que a sessão roda: {said}");
    assert_eq!(project.cargo_calls_after_waiting(1), [the_build_call(&project)]);
    wait_for_the_background_build(&project);
    assert_eq!(project.cargo_calls().len(), 1, "uma compilação só");
}

/// O programa compilado atrás do commit atual pede a mesma compilação, e o
/// aviso diz de qual commit é o programa que a sessão roda até lá.
#[test]
fn a_session_with_the_compiled_program_behind_the_head_starts_the_build_and_names_the_old_commit() {
    let project = Project::as_mustard_source();
    project.compiled_program_says("mustard-rt 0.2.4 (build dev, g0123456789ab 2026-10-02)");

    let said = session_notices(&project);
    assert!(said.contains(&build_notice_head(&project, "session.build.behind", "0123456789ab", "session.build.started")), "{said}");
    assert_eq!(project.cargo_calls_after_waiting(1), [the_build_call(&project)]);
    wait_for_the_background_build(&project);
}

/// O programa compilado no commit atual, com ou sem o `-dirty` do carimbo,
/// deixa a sessão calada e sem compilar nada.
#[test]
fn a_session_with_the_compiled_program_at_the_head_stays_silent_and_builds_nothing() {
    let project = Project::as_mustard_source();
    for stamp in [project.head(), format!("{}-dirty", project.head())] {
        project.compiled_program_says(&format!("mustard-rt 0.2.4 (build dev, g{stamp} 2026-10-02)"));
        let said = session_notices(&project);
        assert!(!said.contains("compilado da branch"), "{stamp}: {said}");
    }
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert!(project.cargo_calls().is_empty(), "em dia: nada compila: {:?}", project.cargo_calls());
}

/// Com outra compilação ainda valendo — a trava dela existe —, a sessão não
/// solta uma segunda e diz que a outra está em andamento.
#[test]
fn a_session_while_another_build_runs_does_not_start_a_second_one() {
    let project = Project::as_mustard_source();
    std::fs::create_dir_all(project.build_marker().parent().expect("the build base")).expect("the base");
    std::fs::write(project.build_marker(), "").expect("the lock of the other build");

    let said = session_notices(&project);
    assert!(said.contains(&build_notice_head(&project, "session.build.missing", "", "session.build.in_progress")), "{said}");
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert!(project.cargo_calls().is_empty(), "a trava vale: {:?}", project.cargo_calls());
}

/// Em qualquer outro projeto a sessão não fala do programa compilado nem
/// compila nada.
#[test]
fn a_session_in_another_project_says_nothing_about_the_compiled_program() {
    let project = Project::new();
    executable::write_executable(&project.bin.join("cargo"), &format!("#!/bin/sh\necho \"$*\" >> '{}'\n", project.cargo_log().display()));

    let said = session_notices(&project);
    assert!(!said.contains("compilado da branch"), "{said}");
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert!(project.cargo_calls().is_empty(), "{:?}", project.cargo_calls());
    assert!(!project.home.join("build").exists(), "nenhuma pasta de compilação nasce");
}

/// Cada arquivo sob `folder`, com o tamanho, a data de modificação e o
/// conteúdo, pelo caminho relativo a ela: a fotografia da pasta que o teste
/// compara antes e depois.
fn snapshot_of(folder: &Path) -> BTreeMap<PathBuf, (u64, Option<std::time::SystemTime>, Vec<u8>)> {
    let mut seen = BTreeMap::new();
    let mut pending = vec![folder.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).expect("the folder reads").flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let meta = std::fs::metadata(&path).expect("the file stats");
            let at = path.strip_prefix(folder).expect("under the folder").to_path_buf();
            seen.insert(at, (meta.len(), meta.modified().ok(), std::fs::read(&path).expect("the file reads")));
        }
    }
    seen
}

/// A versão em construção compila e passa a chamada sem instalar nada: o
/// plugin instalado do registro — a pasta dele, o programa dele e o registro —
/// sai da sessão, da rodada e do fechamento idêntico ao que entrou, e o
/// `cargo` de mentira nunca recebe outra coisa que o `build`.
#[test]
fn building_the_development_version_installs_nothing_and_leaves_the_plugin_folder_untouched() {
    let project = Project::as_mustard_source();
    let config = project.home.join(".claude");
    let plugin = config.join("plugins").join("cache").join("mustard-local").join("mustard").join("0.0.1");
    std::fs::create_dir_all(plugin.join("bin")).expect("the plugin folder");
    executable::write_executable(&plugin.join("bin").join("mustard-rt"), "#!/bin/sh\necho plugin\n");
    std::fs::write(plugin.join("README"), "o plugin instalado\n").expect("a plugin file");
    std::fs::write(
        config.join("plugins").join("installed_plugins.json"),
        json!({"version": 2, "plugins": {"mustard@mustard-local": [
            {"scope": "user", "version": "0.0.1", "installPath": plugin.to_string_lossy()}]}})
        .to_string(),
    )
    .expect("the registry");
    let before = snapshot_of(&config);

    deliver_the_first_wave_changing(&project, "apps/rt/src/main.rs");
    session_notices(&project);
    wait_for_the_background_build(&project);
    let second = project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert!(second.get("commit").is_some(), "the round committed the return: {second}");

    let calls = project.cargo_calls();
    assert_eq!(calls.len(), 2, "uma compilação pela sessão e outra pela rodada: {calls:?}");
    assert_eq!(calls, [the_build_call(&project), changed_build_call(&project)], "full startup then affected runtime build");
    let after = snapshot_of(&config);
    let changed: Vec<&PathBuf> = before.keys().chain(after.keys()).filter(|path| before.get(*path) != after.get(*path)).collect();
    assert!(changed.is_empty(), "o plugin instalado e o registro ficam como estavam: {changed:?}");
}

fn changed_build_call(project: &Project) -> String {
    format!("{} build --profile mustard-dev --locked -p mustard-rt --target-dir {}", project.root.display(), project.build_folder().display())
}
