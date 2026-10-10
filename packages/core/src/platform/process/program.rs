//! O arquivo de um programa no `PATH`, achado como o sistema o acha quando se
//! digita só o nome. No Windows, o `npm`, o `npx` e o `claude` instalado pelo
//! npm são `npm.cmd`, `npx.cmd` e `claude.cmd`, e o [`Command`] do Rust, dado
//! só o nome, completa apenas o `.exe`: sem o arquivo inteiro, ele não acha o
//! programa. Dado o caminho inteiro de um `.cmd` ou de um `.bat`, o Rust o
//! roda pelo `cmd.exe`, com os argumentos escapados.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Os nomes de arquivo com que `program` aparece numa pasta: no Windows, com
/// as extensões de executável que o sistema roda sem que se digite a
/// extensão, nesta ordem; nos outros sistemas, o nome puro. A busca no `PATH`
/// e a busca nas pastas de ferramenta do usuário usam os mesmos nomes.
#[must_use]
pub fn program_file_names(program: &str, windows: bool) -> Vec<String> {
    if windows {
        ["exe", "cmd", "bat"].iter().map(|ext| format!("{program}.{ext}")).collect()
    } else {
        vec![program.to_string()]
    }
}

/// O arquivo de `program` na primeira pasta de `path_env` que tem um dos
/// nomes dele ([`program_file_names`]), provados na ordem dos nomes.
/// `path_env` é uma lista no formato do `PATH`: separada por `;` no Windows
/// (`windows`) e por `:` nos outros sistemas. O nome que já traz separador de
/// pasta é um caminho, e volta como está. `None` quando nenhuma pasta tem o
/// programa.
#[must_use]
pub fn program_file(program: &str, windows: bool, path_env: &str) -> Option<PathBuf> {
    if program.contains('/') || (windows && program.contains('\\')) {
        return Some(PathBuf::from(program));
    }
    let sep = if windows { ';' } else { ':' };
    program_file_in(program, windows, path_env.split(sep))
}

/// O arquivo de `program` na primeira das pastas `dirs` que tem um dos nomes
/// dele ([`program_file_names`]). É a busca de [`program_file`] sem o corte do
/// `PATH`: a pasta que traz o próprio separador do `PATH` no caminho — o `C:`
/// de uma unidade do Windows, sob o separador `:` — não se parte em duas.
fn program_file_in<'a>(program: &str, windows: bool, dirs: impl IntoIterator<Item = &'a str>) -> Option<PathBuf> {
    if program.is_empty() {
        return None;
    }
    let names = program_file_names(program, windows);
    dirs.into_iter()
        .filter(|dir| !dir.is_empty())
        .find_map(|dir| names.iter().map(|name| Path::new(dir).join(name)).find(|file| file.is_file()))
}

/// Um [`Command`] para `program` com o arquivo que o `PATH` desta máquina tem
/// para ele ([`program_file`]). Sem arquivo achado, o nome puro, e o erro ao
/// rodar fica o de sempre.
#[must_use]
pub fn command(program: &str) -> Command {
    match program_location(program) {
        Some(file) => Command::new(file),
        None => Command::new(program),
    }
}

/// Preserve the caller's PATH choice; installed Mustard also carries rg next
/// to its binaries so a fresh machine and plugin cache need no separate install.
#[must_use]
pub fn program_location(program: &str) -> Option<PathBuf> {
    let path_env = std::env::var("PATH").unwrap_or_default();
    if let Some(file) = program_file(program, cfg!(windows), &path_env) {return Some(file);}
    if program != "rg" {return None;}
    let executable = std::env::current_exe().ok().and_then(|path| path.canonicalize().ok());
    location(program, cfg!(windows), "", executable.as_deref())
}

fn location(program: &str, windows: bool, path_env: &str, executable: Option<&Path>) -> Option<PathBuf> {
    program_file(program, windows, path_env).or_else(|| {
        if program != "rg" {return None;}
        let directory = executable?.parent()?;
        program_file_in(program, windows, [directory.to_str()?])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Uma pasta temporária com os arquivos `names`, vazios.
    fn folder_with(names: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for name in names {
            std::fs::write(dir.path().join(name), "").unwrap();
        }
        dir
    }

    fn text(dir: &tempfile::TempDir) -> String {
        dir.path().to_str().unwrap().to_string()
    }

    #[test]
    fn packaged_search_works_without_a_path_but_does_not_replace_a_callers_rg() {
        for windows in [false, true] {
            let name = if windows {"rg.exe"} else {"rg"};
            let package = folder_with(&[name]);
            let caller = folder_with(&[name]);
            let executable = package.path().join("mustard-rt");
            assert_eq!(location("rg", windows, "", Some(&executable)), Some(package.path().join(name)));
            assert_eq!(location("rg", windows, &text(&caller), Some(&executable)), Some(caller.path().join(name)));
            assert_eq!(location("git", windows, "", Some(&executable)), None);
        }
    }

    #[test]
    fn on_windows_the_program_is_found_by_its_cmd_file() {
        let dir = folder_with(&["npm.cmd"]);
        assert_eq!(program_file("npm", true, &text(&dir)), Some(dir.path().join("npm.cmd")));
    }

    /// A pasta é dada inteira, sem passar pelo corte do `PATH`: o caminho de
    /// uma pasta temporária do Windows traz o `C:`, que o separador `:` de
    /// fora do Windows cortaria ao meio, e o teste vale nos três sistemas.
    #[test]
    fn off_windows_only_the_bare_name_counts() {
        let bare = folder_with(&["npm"]);
        assert_eq!(program_file_in("npm", false, [text(&bare).as_str()]), Some(bare.path().join("npm")));
        let cmd_only = folder_with(&["npm.cmd"]);
        assert_eq!(program_file_in("npm", false, [text(&cmd_only).as_str()]), None);
    }

    #[cfg(not(windows))]
    #[test]
    fn off_windows_the_path_splits_on_colons_and_the_first_folder_wins() {
        let first = folder_with(&["npm"]);
        let second = folder_with(&["npm"]);
        let cmd_only = folder_with(&["npm.cmd"]);
        let path_env = format!("{}:{}:{}", text(&cmd_only), text(&first), text(&second));
        assert_eq!(program_file("npm", false, &path_env), Some(first.path().join("npm")));
        assert_eq!(program_file("npm", false, &text(&cmd_only)), None);
    }

    /// O que o servidor do Windows mostrou: o caminho da pasta com `C:` não
    /// cabe numa lista separada por `:`. Num sistema que aceita `:` no nome da
    /// pasta, a mesma pasta fica inteira quando dada à busca e é cortada ao
    /// meio quando vai dentro do `PATH`.
    #[cfg(not(windows))]
    #[test]
    fn a_folder_with_a_colon_in_its_path_is_searched_whole_and_cut_in_a_path_list() {
        let dir = tempfile::Builder::new().prefix("C:").tempdir().unwrap();
        std::fs::write(dir.path().join("npm"), "").unwrap();
        assert_eq!(program_file_in("npm", false, [text(&dir).as_str()]), Some(dir.path().join("npm")));
        assert_eq!(program_file("npm", false, &text(&dir)), None);
    }

    #[test]
    fn the_exe_comes_before_the_cmd_in_the_same_folder() {
        let dir = folder_with(&["node.cmd", "node.exe"]);
        assert_eq!(program_file("node", true, &text(&dir)), Some(dir.path().join("node.exe")));
    }

    #[test]
    fn on_windows_the_path_splits_on_semicolons_and_the_first_folder_wins() {
        let first = folder_with(&["npm.cmd"]);
        let second = folder_with(&["npm.cmd"]);
        let path_env = format!("{};{}", text(&first), text(&second));
        assert_eq!(program_file("npm", true, &path_env), Some(first.path().join("npm.cmd")));
    }

    #[test]
    fn a_name_with_a_folder_separator_comes_back_as_written() {
        let empty = folder_with(&[]);
        assert_eq!(program_file("bin/npm", false, &text(&empty)), Some(PathBuf::from("bin/npm")));
        assert_eq!(program_file(r"C:\node\npm.cmd", true, &text(&empty)), Some(PathBuf::from(r"C:\node\npm.cmd")));
    }
}
