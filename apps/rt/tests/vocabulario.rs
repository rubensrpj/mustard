//! O vocabulário do Mustard: a lista das tarefas que ainda não viraram onda
//! se chama backlog, o nome que times de desenvolvimento já usam, em pt-BR e
//! em inglês. A palavra antiga, nas duas línguas, não volta em nenhum arquivo
//! de `apps/` nem de `packages/`: nem nos textos que o binário imprime, nem
//! nos comentários, nem dentro de um nome do código.
//!
//! Os arquivos são os que o git conhece na cópia — os comitados e os novos
//! ainda não comitados, fora os que o `.gitignore` deixa de fora —, sem as
//! pastas de compilação, de pacote e de fixtures. Este arquivo fica de fora:
//! o nome do próprio teste diz a palavra que ele procura.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A palavra antiga, em pt-BR e em inglês, no singular e no plural. Montada
/// em pedaços para este arquivo não precisar dizê-la inteira.
fn old_words() -> [String; 4] {
    let pt = format!("{}{}", "ces", "ta");
    let en = format!("{}{}", "bas", "ket");
    [pt.clone(), format!("{pt}s"), en.clone(), format!("{en}s")]
}

/// As pastas que nunca são lidas: saída de compilação, pacote gerado,
/// dependência baixada e fixtures dos testes.
const SKIPPED_DIRS: &[&str] = &["target", "dist", "node_modules", "fixtures"];

/// A raiz do repositório, a partir deste crate (`apps/rt`).
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Os arquivos de `apps/` e `packages/` que o git conhece, em ordem.
fn repo_files(root: &Path) -> Vec<PathBuf> {
    let out = Command::new("git")
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
            "apps",
            "packages",
        ])
        .current_dir(root)
        .output()
        .expect("o git lista os arquivos do repositório");
    assert!(
        out.status.success(),
        "git ls-files falhou: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut files: Vec<PathBuf> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(PathBuf::from)
        .filter(|path| {
            !path
                .components()
                .any(|c| SKIPPED_DIRS.contains(&c.as_os_str().to_string_lossy().as_ref()))
        })
        .filter(|path| !path.ends_with("apps/rt/tests/vocabulario.rs"))
        .collect();
    files.sort();
    files.dedup();
    files
}

/// As palavras de uma linha, com cada nome do código partido nas partes
/// dele: `backlog_left` dá `backlog` e `left`, `dispatchBacklog` dá
/// `dispatch` e `backlog`, `HTTPServer` dá `http` e `server`. Tudo em
/// minúsculas.
fn words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    for run in line.split(|c: char| !c.is_alphanumeric()) {
        let chars: Vec<char> = run.chars().collect();
        let mut start = 0;
        for i in 1..chars.len() {
            let (prev, cur) = (chars[i - 1], chars[i]);
            let next_lower = chars.get(i + 1).is_some_and(|c| c.is_lowercase());
            let boundary = (prev.is_lowercase() && cur.is_uppercase())
                || (prev.is_uppercase() && cur.is_uppercase() && next_lower)
                || (prev.is_alphabetic() != cur.is_alphabetic());
            if boundary {
                out.push(chars[start..i].iter().collect::<String>().to_lowercase());
                start = i;
            }
        }
        if start < chars.len() {
            out.push(chars[start..].iter().collect::<String>().to_lowercase());
        }
    }
    out
}

/// Cada linha de `text` que diz a palavra antiga, como `arquivo:linha: texto`.
fn hits(file: &Path, text: &str, banned: &[String]) -> Vec<String> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| words(line).iter().any(|w| banned.contains(w)))
        .map(|(n, line)| format!("{}:{}: {}", file.display(), n + 1, line.trim()))
        .collect()
}

/// A palavra antiga não aparece em nenhum arquivo de `apps/` nem de
/// `packages/`, inteira ou dentro de um nome do código; a falha diz o
/// arquivo e a linha de cada vez que ela aparece.
#[test]
fn nenhum_arquivo_do_mustard_diz_cesta() {
    let root = repo_root();
    let banned = old_words();
    let files = repo_files(&root);
    assert!(
        files.len() > 100,
        "a lista dos arquivos veio curta demais: {}",
        files.len()
    );
    let mut found = Vec::new();
    for file in &files {
        // Arquivo apagado na cópia, ou que não é texto, não tem o que ler.
        let Ok(bytes) = std::fs::read(root.join(file)) else {
            continue;
        };
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        found.extend(hits(file, &text, &banned));
    }
    assert!(
        found.is_empty(),
        "a palavra antiga voltou em {} linha(s):\n{}",
        found.len(),
        found.join("\n")
    );
}

/// A leitura das palavras acha a palavra antiga dentro de um nome do código,
/// em qualquer caixa, e não acha quando ela só aparece colada no meio de
/// outra palavra, como em `replaceState`.
#[test]
fn a_leitura_acha_a_palavra_dentro_de_um_nome_do_codigo() {
    let banned = old_words();
    let [pt, _, en, _] = &banned;
    let upper = |w: &str| format!("{}{}", w[..1].to_uppercase(), &w[1..]);
    for line in [
        format!("fn {en}_left() {{}}"),
        format!("let dispatch{} = 1;", upper(en)),
        format!("// a {pt} ainda tem tarefa"),
        format!("struct {}Task;", upper(en)),
        format!("const {}: u8 = 0;", en.to_uppercase()),
        format!("\"s-{pt}-lotes\""),
    ] {
        assert_eq!(
            hits(Path::new("x.rs"), &line, &banned).len(),
            1,
            "não achou em: {line}"
        );
    }
    for line in [
        "history.replaceState(null, '', '#' + id);",
        "a lista das tarefas do backlog",
    ] {
        assert!(
            hits(Path::new("x.rs"), line, &banned).is_empty(),
            "achou onde não tem: {line}"
        );
    }
}
