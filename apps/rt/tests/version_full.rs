//! `apps/rt/build.rs` monta `MUSTARD_VERSION_FULL` — a linha que
//! `mustard-rt --version` mostra — a partir do `git` do repositório onde ele
//! compila. O pacote Linux (`packaging/linux/build-deb.sh`) copia o código
//! para uma pasta de build SEM a pasta `.git` antes de compilar; ali o `git`
//! não acha o commit, e a versão sairia só com o número. Por isso o script
//! lê o commit, a marca de mudança (dirty) e a data no repositório ORIGINAL,
//! antes da cópia, e os entrega ao build por variável de ambiente
//! (`MUSTARD_GIT_HASH`, `MUSTARD_GIT_DIRTY`, `MUSTARD_GIT_DATE`) — que
//! `git_describe`, em `apps/rt/build.rs`, lê antes de tentar o `git`.
//!
//! Para provar isso sem compilar o `mustard-rt` inteiro (todas as suas
//! dependências), o `build.rs` — um programa Rust comum, sem dependências
//! além da std — é compilado sozinho com `rustc`, do jeito que o cargo já faz
//! de verdade com todo script de build, e rodado com o diretório de trabalho
//! fora de qualquer repositório git, para as chamadas internas a `git`
//! falharem de propósito, como aconteceria dentro da pasta de build do
//! pacote Linux. A prova é a linha `cargo:rustc-env=MUSTARD_VERSION_FULL=…`
//! que ele imprime — a mesma linha que o cargo lê para gravar a variável de
//! compilação que os binários embutem com `env!(...)`.
//!
//! O carimbo diz também QUAL código sujo havia: `MUSTARD_GIT_DIFF`, o resumo
//! de `git diff HEAD` mais os arquivos novos que o git não ignora (vazio com a
//! pasta limpa). O `-dirty` vale para os dois, o arquivo rastreado mudado e o
//! arquivo novo, e dois códigos sujos de jeitos diferentes têm resumos
//! diferentes.

#[path = "support/manifest_dir.rs"]
mod manifest_dir;

use std::path::{Path, PathBuf};
use std::process::Command;

/// A raiz do repositório, a partir deste crate (`apps/rt`).
fn repo_root() -> PathBuf {
    manifest_dir::manifest_dir()
        .join("../..")
        .canonicalize()
        .expect("repo root resolves")
}

/// Compila `apps/rt/build.rs` sozinho — sem depender do resto do workspace,
/// do mesmo jeito que o cargo compila todo script de build antes de rodá-lo.
fn compile_build_rs(out_bin: &Path) {
    let build_rs = repo_root().join("apps/rt/build.rs");
    let out = Command::new("rustc")
        .arg(&build_rs)
        .arg("-o")
        .arg(out_bin)
        .output()
        .expect("rustc runs");
    assert!(out.status.success(), "rustc não compilou {}: {}", build_rs.display(), String::from_utf8_lossy(&out.stderr));
}

/// Roda o `build.rs` já compilado, com o diretório de trabalho fora de
/// qualquer repositório git (`/tmp`, sem `.git` acima) — para as chamadas a
/// `git` de dentro dele falharem de propósito, como falhariam na pasta de
/// build do pacote Linux, que não tem `.git`. Devolve a linha
/// `cargo:rustc-env=MUSTARD_VERSION_FULL=…` que ele imprime.
fn run_build_rs_without_git(bin: &Path, cwd: &Path, extra_env: &[(&str, &str)]) -> String {
    let mut cmd = Command::new(bin);
    cmd.current_dir(cwd).env_clear().env("CARGO_PKG_VERSION", "9.9.9");
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("the compiled build.rs runs");
    assert!(out.status.success(), "build.rs saiu com erro: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    stdout
        .lines()
        .find(|l| l.starts_with("cargo:rustc-env=MUSTARD_VERSION_FULL="))
        .unwrap_or_else(|| panic!("sem a linha MUSTARD_VERSION_FULL: {stdout}"))
        .trim_start_matches("cargo:rustc-env=MUSTARD_VERSION_FULL=")
        .to_string()
}

/// Sem `.git` e sem as variáveis: a versão sai só com o número, como hoje —
/// o `git` de dentro do build.rs não acha nada nesse diretório, e não há
/// variável para substituir.
#[test]
fn without_the_environment_and_without_git_the_version_is_just_the_number() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let bin = tmp.path().join("build_rs_bin");
    compile_build_rs(&bin);
    let empty_cwd = tmp.path().join("no-git-here");
    std::fs::create_dir_all(&empty_cwd).expect("mkdir");

    let full = run_build_rs_without_git(&bin, &empty_cwd, &[]);
    assert_eq!(full, "9.9.9", "sem git e sem variável, a versão tem de ficar só com o número: {full}");
}

/// Com `.git` ausente (o cenário do pacote Linux) mas com o commit, a marca
/// de mudança e a data entregues por variável de ambiente, a versão mostra o
/// commit — exatamente o que `packaging/linux/build-deb.sh` passa a fazer. A
/// versão da publicação chega junto, como chega na esteira, e o bloco do
/// commit continua na linha.
#[test]
fn the_version_shows_the_commit_from_the_environment() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let bin = tmp.path().join("build_rs_bin");
    compile_build_rs(&bin);
    let empty_cwd = tmp.path().join("no-git-here");
    std::fs::create_dir_all(&empty_cwd).expect("mkdir");

    let full = run_build_rs_without_git(
        &bin,
        &empty_cwd,
        &[
            ("MUSTARD_RELEASE_VERSION", "9.9.9"),
            ("MUSTARD_GIT_HASH", "deadbeef1234"),
            ("MUSTARD_GIT_DIRTY", "1"),
            ("MUSTARD_GIT_DATE", "2026-01-02"),
        ],
    );
    assert_eq!(full, "9.9.9 (build dev, gdeadbeef1234-dirty 2026-01-02)", "a versão tem de trazer o commit da variável de ambiente: {full}");
}

/// O `git` em `dir`, com quem comita já dito, e a saída sem as bordas.
fn git_in(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Os arquivos do núcleo que os scripts de build incluem por caminho,
/// relativos à raiz do repositório. Os dois scripts incluem o `sha256.rs` e o
/// `tree_state.rs`; o `hooks_digest.rs` entra só no do `mustard-rt` (e no do
/// scan, que inclui o script do `mustard-rt` inteiro).
const SHARED_BY_PATH: [&str; 3] = ["packages/core/src/io/sha256.rs", "packages/core/src/io/tree_state.rs", "packages/core/src/io/hooks_digest.rs"];

/// Um projeto git em `root` com dois pacotes, cada um com um dos dois
/// scripts de build de verdade — o do `mustard-rt` e o do `mustard` — e um
/// binário que só imprime a versão que o script carimbou.
///
/// O projeto traz as pastas do plugin que o script do `mustard-rt` vigia, a
/// dos ganchos e a da interface, com o tema. O cargo roda de novo, a cada
/// compilação, o script que vigia um caminho que não existe; com as pastas no
/// lugar, ele só roda quando algo vigiado muda, e a nova compilação depois da
/// troca de commit prova a vigia do commit.
fn stamped_project(root: &Path) {
    let write = |path: &str, body: &str| {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("a parent folder")).expect("mkdir");
        std::fs::write(path, body).expect("write");
    };
    for shared in SHARED_BY_PATH {
        write(shared, &std::fs::read_to_string(repo_root().join(shared)).expect("a file the scripts include"));
    }
    write("Cargo.toml", "[workspace]\nmembers = [\"apps/rt\", \"apps/cli\"]\nresolver = \"2\"\n");
    for (crate_dir, name) in [("apps/rt", "carimbo-rt"), ("apps/cli", "carimbo-cli")] {
        let manifest = format!("[package]\nname = \"{name}\"\nversion = \"9.9.9\"\nedition = \"2021\"\n");
        write(&format!("{crate_dir}/Cargo.toml"), &manifest);
        let script = std::fs::read_to_string(repo_root().join(crate_dir).join("build.rs")).expect("the build script");
        write(&format!("{crate_dir}/build.rs"), &script);
        write(&format!("{crate_dir}/src/main.rs"), "fn main() {\n    println!(\"{}\", env!(\"MUSTARD_VERSION_FULL\"));\n}\n");
    }
    write("plugin/hooks/hooks.json", "{}\n");
    write("plugin/ui/theme.js", "export const theme = {};\n");
    write(".gitignore", "target/\nCargo.lock\n");
    write("LEIAME.md", "um\n");
    git_in(root, &["init", "-q"]);
    git_in(root, &["add", "-A"]);
    git_in(root, &["commit", "-q", "-m", "primeiro"]);
}

/// Compila o projeto de teste na cópia `copy`, com a pasta de compilação
/// dentro dela, pelo mesmo cargo que roda os testes, e devolve a versão que
/// cada binário imprime.
fn build_and_read_versions(copy: &Path) -> Vec<String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut build = Command::new(cargo);
    build.args(["build", "--offline", "--quiet"]).current_dir(copy).env("CARGO_TARGET_DIR", copy.join("target"));
    for var in ["MUSTARD_BUILD_NUMBER", "MUSTARD_GIT_HASH", "MUSTARD_GIT_DIRTY", "MUSTARD_GIT_DATE", "MUSTARD_GIT_DIFF", "GIT_DIR", "GIT_WORK_TREE"] {
        build.env_remove(var);
    }
    let out = build.output().expect("cargo runs");
    assert!(out.status.success(), "cargo build: {}", String::from_utf8_lossy(&out.stderr));
    ["carimbo-rt", "carimbo-cli"]
        .iter()
        .map(|name| {
            let bin = copy.join("target").join("debug").join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
            let out = Command::new(&bin).output().expect("the stamped binary runs");
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        })
        .collect()
}

/// Na cópia ligada ao projeto (`git worktree add`), com a compilação dentro
/// dela, a versão acompanha o commit da cópia: compilada num commit, levada a
/// outro com `checkout --detach` — que só muda o arquivo que o commit mudou,
/// e nenhum dos pacotes — e compilada de novo, a versão dos dois binários
/// mostra o commit novo.
#[test]
fn a_linked_copy_moved_to_another_commit_stamps_the_new_commit() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("projeto");
    std::fs::create_dir_all(&root).expect("mkdir");
    stamped_project(&root);
    let copy = tmp.path().join("vaga");
    git_in(&root, &["worktree", "add", "-q", "--detach", &copy.to_string_lossy(), "HEAD"]);

    let first = git_in(&copy, &["rev-parse", "--short=12", "HEAD"]);
    for version in build_and_read_versions(&copy) {
        assert!(version.contains(&format!("g{first} ")), "a primeira compilação carimba o commit da cópia: {version}");
    }

    std::fs::write(root.join("LEIAME.md"), "dois\n").expect("write");
    git_in(&root, &["commit", "-q", "-am", "segundo"]);
    let second = git_in(&root, &["rev-parse", "--short=12", "HEAD"]);
    git_in(&copy, &["checkout", "-q", "--detach", "--force", &second]);

    for version in build_and_read_versions(&copy) {
        assert!(version.contains(&format!("g{second} ")), "a cópia levada a outro commit carimba o novo: {version}");
    }
}

/// Roda o `build.rs` já compilado com o diretório de trabalho em `cwd` — um
/// repositório de verdade, onde o `git` acha o commit — e devolve a versão e o
/// resumo do que estava por comitar que ele imprime. As variáveis do pacote
/// Linux não valem aqui: é o `git` quem responde.
fn run_build_rs_in_repository(bin: &Path, cwd: &Path) -> (String, String) {
    let mut cmd = Command::new(bin);
    cmd.current_dir(cwd).env("CARGO_PKG_VERSION", "9.9.9");
    for var in ["MUSTARD_BUILD_NUMBER", "MUSTARD_GIT_HASH", "MUSTARD_GIT_DIRTY", "MUSTARD_GIT_DATE", "MUSTARD_GIT_DIFF", "GIT_DIR", "GIT_WORK_TREE"] {
        cmd.env_remove(var);
    }
    let out = cmd.output().expect("the compiled build.rs runs");
    assert!(out.status.success(), "build.rs saiu com erro: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let line = |key: &str| {
        let prefix = format!("cargo:rustc-env={key}=");
        stdout
            .lines()
            .find_map(|l| l.strip_prefix(&prefix))
            .unwrap_or_else(|| panic!("sem a linha {key}: {stdout}"))
            .to_string()
    };
    (line("MUSTARD_VERSION_FULL"), line("MUSTARD_GIT_DIFF"))
}

/// Um projeto git de um commit só, e o `build.rs` do `mustard-rt` compilado
/// para rodar nele: a pasta do projeto, a do script e o programa.
fn committed_project_with_build_script() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("projeto");
    std::fs::create_dir_all(&root).expect("mkdir");
    stamped_project(&root);
    let bin = tmp.path().join("build_rs_bin");
    compile_build_rs(&bin);
    (tmp, root, bin)
}

/// Com a árvore limpa a versão não leva `-dirty` e o resumo vem vazio.
#[test]
fn a_clean_tree_has_no_dirty_mark_and_an_empty_diff() {
    let (_tmp, root, bin) = committed_project_with_build_script();
    let (full, diff) = run_build_rs_in_repository(&bin, &root);
    assert!(full.contains("(build dev, g") && !full.contains("-dirty"), "árvore limpa, sem -dirty: {full}");
    assert_eq!(diff, "", "árvore limpa, sem resumo");
}

/// Um arquivo rastreado alterado e um arquivo novo que o git não ignora sujam
/// a árvore, e cada um dá o seu resumo: dois códigos diferentes nunca passam
/// pelo mesmo.
#[test]
fn a_changed_tracked_file_and_a_new_file_are_dirty_with_different_diffs() {
    let (_tmp, root, bin) = committed_project_with_build_script();

    std::fs::write(root.join("LEIAME.md"), "dois\n").expect("write");
    let (tracked_full, tracked_diff) = run_build_rs_in_repository(&bin, &root);
    git_in(&root, &["checkout", "-q", "--", "LEIAME.md"]);
    let (clean_full, clean_diff) = run_build_rs_in_repository(&bin, &root);

    std::fs::write(root.join("novo.md"), "dois\n").expect("write");
    let (new_full, new_diff) = run_build_rs_in_repository(&bin, &root);

    assert!(tracked_full.contains("-dirty"), "arquivo rastreado alterado suja: {tracked_full}");
    assert!(new_full.contains("-dirty"), "arquivo novo não rastreado suja também: {new_full}");
    assert!(!tracked_diff.is_empty() && !new_diff.is_empty(), "os dois têm resumo");
    assert_ne!(tracked_diff, new_diff, "um arquivo alterado e um arquivo novo são códigos diferentes");
    assert!(!clean_full.contains("-dirty") && clean_diff.is_empty(), "desfeita a mudança, a árvore volta a limpa: {clean_full}");
}

/// O pacote Linux copia o código sem `.git`: o resumo, como o commit, vem da
/// variável de ambiente que o script do pacote lê antes da cópia.
#[test]
fn the_diff_comes_from_the_environment_when_git_is_absent() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let bin = tmp.path().join("build_rs_bin");
    compile_build_rs(&bin);
    let empty_cwd = tmp.path().join("no-git-here");
    std::fs::create_dir_all(&empty_cwd).expect("mkdir");

    let mut cmd = Command::new(&bin);
    cmd.current_dir(&empty_cwd).env_clear().env("CARGO_PKG_VERSION", "9.9.9");
    for (k, v) in [("MUSTARD_GIT_HASH", "deadbeef1234"), ("MUSTARD_GIT_DIRTY", "1"), ("MUSTARD_GIT_DATE", "2026-01-02"), ("MUSTARD_GIT_DIFF", "0123456789ab")] {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("the compiled build.rs runs");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("cargo:rustc-env=MUSTARD_GIT_DIFF=0123456789ab"), "{stdout}");
    assert!(stdout.contains("cargo:rerun-if-env-changed=MUSTARD_GIT_DIFF"), "{stdout}");
}

/// O script do pacote Linux, carregado por `source` (só as funções ficam
/// definidas), com o corpo `body` rodado em `bash` em cima delas, com as
/// variáveis `env` a mais; devolve o que o `bash` deixou, saia ele como sair.
/// O `PATH` de `path_first`, quando vem, passa à frente.
#[cfg(unix)]
fn package_script(body: &str, args: &[&Path], path_first: Option<&Path>, env: &[(&str, &str)]) -> std::process::Output {
    let script = repo_root().join("packaging/linux/build-deb.sh");
    let mut cmd = Command::new("bash");
    cmd.arg("-c").arg(format!("source \"$1\"; shift; {body}")).arg("_").arg(&script).args(args);
    for var in ["MUSTARD_BUILD_NUMBER", "MUSTARD_GIT_HASH", "MUSTARD_GIT_DIRTY", "MUSTARD_GIT_DATE", "MUSTARD_GIT_DIFF", "GIT_DIR", "GIT_WORK_TREE"] {
        cmd.env_remove(var);
    }
    for (key, value) in env {
        cmd.env(key, value);
    }
    if let Some(first) = path_first {
        let rest = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![first.to_path_buf()];
        paths.extend(std::env::split_paths(&rest));
        cmd.env("PATH", std::env::join_paths(paths).expect("a PATH"));
    }
    cmd.output().expect("bash runs")
}

/// [`package_script`] sem variáveis a mais, que tem de sair sem erro; devolve
/// a saída comum.
#[cfg(unix)]
fn run_package_script(body: &str, args: &[&Path], path_first: Option<&Path>) -> String {
    let out = package_script(body, args, path_first, &[]);
    assert!(out.status.success(), "o script do pacote saiu com erro: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// O que o script do pacote imprime de cada uma das quatro variáveis do
/// commit, uma por linha.
#[cfg(unix)]
const PRINT_STAMP: &str = "printf '%s\\n%s\\n%s\\n%s\\n' \"$MUSTARD_GIT_HASH\" \"$MUSTARD_GIT_DIRTY\" \"$MUSTARD_GIT_DATE\" \"$MUSTARD_GIT_DIFF\"";

/// Um repositório git de um commit só em `dir`.
#[cfg(unix)]
fn one_commit_repository(dir: &Path) {
    std::fs::create_dir_all(dir).expect("mkdir");
    git_in(dir, &["init", "-q"]);
    std::fs::write(dir.join("a.md"), "a\n").expect("write");
    git_in(dir, &["add", "-A"]);
    git_in(dir, &["commit", "-q", "-m", "um"]);
}

/// O que o script do pacote lê da pasta `repo`: o commit, se está suja (`1` ou
/// vazio), a data e o resumo, nesta ordem.
#[cfg(unix)]
fn package_script_stamp(repo: &Path) -> Vec<String> {
    run_package_script(&format!("read_git_stamp \"$1\"; {PRINT_STAMP}"), &[repo], None).lines().map(str::to_string).collect()
}

/// O repositório que o contêiner do pacote monta é de outro dono — o da
/// máquina —, e o contêiner roda como root: o git recusa ler a pasta de outro
/// dono. O script lê o commit e a data mesmo assim, em vez de deixar o
/// carimbo vazio em silêncio. A variável de teste do próprio git faz ele tratar
/// a pasta como de outro dono, sem precisar de root.
#[cfg(unix)]
#[test]
fn the_package_script_reads_the_commit_of_a_repository_of_another_owner() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("de-outro-dono");
    one_commit_repository(&root);
    let commit = git_in(&root, &["rev-parse", "--short=12", "HEAD"]);
    let date = git_in(&root, &["log", "-1", "--format=%cs"]);
    // Even a folder of another owner passes when the machine's own git
    // configuration lists it, or every folder, as safe: the test reads an
    // empty one instead.
    let empty = tmp.path().join("gitconfig");
    std::fs::write(&empty, "").expect("empty git configuration");
    let empty = empty.to_str().expect("utf-8 path");
    let other_owner = [("GIT_TEST_ASSUME_DIFFERENT_OWNER", "1"), ("GIT_CONFIG_GLOBAL", empty)];
    let refused = Command::new("git").args(["rev-parse", "HEAD"]).current_dir(&root).envs(other_owner).output().expect("git runs");
    assert!(!refused.status.success(), "o git desta máquina não simula a pasta de outro dono; o teste não provaria nada");

    let out = package_script(&format!("read_git_stamp \"$1\"; {PRINT_STAMP}"), &[&root], None, &other_owner);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let stamp: Vec<String> = String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).collect();
    assert_eq!(stamp, [commit.as_str(), "", date.as_str(), ""], "a pasta de outro dono");
}

/// Sem commit lido, o pacote não sai: a leitura que o script faz antes de
/// compilar recusa e mostra o motivo do git, em vez de embalar binários que
/// respondem só o número da versão. Com o commit, ela segue.
#[cfg(unix)]
#[test]
fn the_package_build_refuses_a_folder_without_a_commit() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let plain = tmp.path().join("sem-git");
    std::fs::create_dir_all(&plain).expect("mkdir");
    std::fs::write(plain.join("a.md"), "a\n").expect("write");
    let body = "require_git_stamp \"$1\"; echo seguiu";
    let english = [("LC_ALL", "C")];

    let refused = package_script(body, &[&plain], None, &english);
    let (stdout, stderr) = (String::from_utf8_lossy(&refused.stdout), String::from_utf8_lossy(&refused.stderr));
    assert!(!refused.status.success() && !stdout.contains("seguiu"), "a pasta sem commit seguiu: {stdout}{stderr}");
    assert!(stderr.contains(&plain.display().to_string()), "a recusa não diz a pasta: {stderr}");
    assert!(stderr.contains("not a git repository"), "a recusa não traz o motivo do git: {stderr}");

    let repo = tmp.path().join("com-commit");
    one_commit_repository(&repo);
    let went = package_script(body, &[&repo], None, &english);
    assert!(went.status.success() && String::from_utf8_lossy(&went.stdout).contains("seguiu"), "{}", String::from_utf8_lossy(&went.stderr));
}

/// O script do pacote e o `build.rs` leem o mesmo estado da mesma pasta: o
/// mesmo commit, o mesmo sujo e o mesmo resumo, com a pasta limpa, com um
/// arquivo rastreado mudado, com SÓ um arquivo novo (que o `git diff --quiet`
/// de antes não via) e com os dois.
#[cfg(unix)]
#[test]
fn the_package_script_reads_the_same_state_as_the_build_script() {
    let (_tmp, root, bin) = committed_project_with_build_script();
    let commit = git_in(&root, &["rev-parse", "--short=12", "HEAD"]);
    let date = git_in(&root, &["log", "-1", "--format=%cs"]);
    let mut seen: Vec<String> = Vec::new();
    for (name, setup) in [
        ("limpa", Box::new(|_: &Path| {}) as Box<dyn Fn(&Path)>),
        ("rastreado mudado", Box::new(|root: &Path| std::fs::write(root.join("LEIAME.md"), "dois\n").expect("write"))),
        ("só arquivo novo", Box::new(|root: &Path| std::fs::write(root.join("novo.md"), "texto novo\n").expect("write"))),
        ("os dois", Box::new(|root: &Path| {
            std::fs::write(root.join("LEIAME.md"), "tres  \n\n").expect("write");
            std::fs::create_dir_all(root.join("pasta")).expect("mkdir");
            std::fs::write(root.join("pasta/b.md"), "b\n").expect("write");
            std::fs::write(root.join("a.md"), "a\n").expect("write");
        })),
    ] {
        git_in(&root, &["checkout", "-q", "--", "."]);
        git_in(&root, &["clean", "-q", "-f", "-d"]);
        setup(&root);
        let (full, diff) = run_build_rs_in_repository(&bin, &root);
        let stamp = package_script_stamp(&root);
        let dirty = if full.contains("-dirty") { "1" } else { "" };
        assert_eq!(stamp, [commit.as_str(), dirty, date.as_str(), diff.as_str()], "pasta {name}");
        seen.push(diff);
    }
    assert!(seen[0].is_empty() && seen[1..].iter().all(|diff| !diff.is_empty()), "só a pasta limpa tem resumo vazio: {seen:?}");
    assert_eq!(seen.len(), seen.iter().collect::<std::collections::BTreeSet<_>>().len(), "cada estado tem o seu resumo: {seen:?}");
}

/// Uma pasta que não é repositório, ou um repositório sem commit, deixa as
/// quatro variáveis vazias: o script não afirma nada sobre o que não leu.
#[cfg(unix)]
#[test]
fn the_package_script_leaves_the_stamp_empty_without_a_repository_or_a_commit() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let plain = tmp.path().join("sem-git");
    std::fs::create_dir_all(&plain).expect("mkdir");
    std::fs::write(plain.join("a.md"), "a\n").expect("write");
    assert_eq!(package_script_stamp(&plain), ["", "", "", ""], "uma pasta fora de repositório");
    let empty = tmp.path().join("sem-commit");
    std::fs::create_dir_all(&empty).expect("mkdir");
    git_in(&empty, &["init", "-q"]);
    std::fs::write(empty.join("a.md"), "a\n").expect("write");
    assert_eq!(package_script_stamp(&empty), ["", "", "", ""], "um repositório sem commit");
}

/// O `cargo build` do pacote recebe as quatro variáveis, lidas do repositório
/// original: o commit, o sujo, a data e o resumo de um código que só tinha um
/// arquivo novo. O `cargo` de mentira só escreve o que viu.
#[cfg(unix)]
#[test]
fn the_package_build_hands_the_four_variables_to_cargo() {
    use std::os::unix::fs::PermissionsExt;
    let (tmp, root, bin) = committed_project_with_build_script();
    std::fs::write(root.join("novo.md"), "texto novo\n").expect("write");
    let (_full, diff) = run_build_rs_in_repository(&bin, &root);
    let fake = tmp.path().join("fake-bin");
    std::fs::create_dir_all(&fake).expect("mkdir");
    let cargo = fake.join("cargo");
    std::fs::write(&cargo, "#!/bin/sh\nenv | grep '^MUSTARD_' | sort\necho \"args: $*\"\n").expect("write");
    std::fs::set_permissions(&cargo, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let build_area = tmp.path().join("area");
    std::fs::create_dir_all(&build_area).expect("mkdir");
    let body = "BUILD=\"$2\"; CARGO_TARGET=\"$2/t\"; VERSION=1.2.3; read_git_stamp \"$1\"; build_cli";
    let seen = run_package_script(body, &[&root, &build_area], Some(&fake));
    let commit = git_in(&root, &["rev-parse", "--short=12", "HEAD"]);
    let date = git_in(&root, &["log", "-1", "--format=%cs"]);
    for expected in [
        format!("MUSTARD_GIT_HASH={commit}"),
        "MUSTARD_GIT_DIRTY=1".to_string(),
        format!("MUSTARD_GIT_DATE={date}"),
        format!("MUSTARD_GIT_DIFF={diff}"),
        "MUSTARD_RELEASE_VERSION=1.2.3".to_string(),
        "args: build --release --locked --bin scan --bin mustard-rt --bin mustard".to_string(),
    ] {
        assert!(seen.lines().any(|line| line == expected), "falta `{expected}` no que o cargo recebeu:\n{seen}");
    }
}

/// O teste de instalação da esteira (`packaging/smoke-install.py`) confere o
/// commit nos três programas compilados: o programa cuja linha de versão não
/// traz `g<commit>` para o teste, com o nome dele e o que respondeu; com o
/// commit nos três, o teste segue adiante. Os programas de mentira só
/// respondem `--version`, e o teste para no passo seguinte, a instalação.
#[cfg(unix)]
#[test]
fn the_install_smoke_requires_the_commit_in_the_three_compiled_programs() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().expect("tempdir");
    let bin = tmp.path().join("bin");
    std::fs::create_dir_all(&bin).expect("mkdir");
    for name in ["COPYING", "LICENSE-MIT", "UNLICENSE"] {
        std::fs::write(bin.join(format!("ripgrep-{name}")), "licença\n").expect("write");
    }
    let program = |name: &str, line: &str| {
        let path = bin.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n[ \"$1\" = --version ] && echo '{line}' && exit 0\nexit 3\n")).expect("write");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    };
    let smoke = || {
        let out = Command::new("python3")
            .arg(repo_root().join("packaging/smoke-install.py"))
            .arg("--bin-dir")
            .arg(&bin)
            .args(["--version", "0.2.21", "--commit", "0123456789ab"])
            .output()
            .expect("python3 runs");
        assert!(!out.status.success(), "os programas de mentira nunca passam o teste inteiro");
        String::from_utf8_lossy(&out.stderr).into_owned()
    };
    program("rtk", "rtk 0.49.0");
    program("rg", "ripgrep 15.2.0");
    for name in ["mustard", "mustard-rt", "scan"] {
        program(name, &format!("{name} 0.2.21"));
    }
    let bare = smoke();
    assert!(bare.contains("installed commit differs: mustard answered 'mustard 0.2.21'"), "{bare}");
    let stamped = "0.2.21 (build 7, g0123456789ab 2026-10-14)";
    program("mustard", &format!("mustard {stamped}"));
    program("mustard-rt", &format!("mustard-rt {stamped}"));
    let without_scan = smoke();
    assert!(without_scan.contains("installed commit differs: scan answered 'scan 0.2.21'"), "{without_scan}");
    program("scan", &format!("grain {stamped}"));
    let all_stamped = smoke();
    assert!(!all_stamped.contains("installed commit differs") && all_stamped.contains("'run', 'upsert'] failed"), "{all_stamped}");
}
