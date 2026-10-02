//! A prova de versão de uma medida: de que código saiu o programa que mediu e
//! de que compilação do scan saiu cada mapa que ele abriu.
//!
//! Um número de medida só vale com a versão que o gerou. A régua grava, junto
//! do resultado, o commit medido, se havia código por comitar (e um resumo do
//! que era), o SHA-256 do programa que rodou e a marca de cada mapa que abriu.
//! Um mapa feito por outra compilação do scan é recusado antes de qualquer
//! número sair: o banco guarda em cada bloco a marca de quem o encheu, e a
//! marca esperada é a que o `scan` compilado com o código medido diz em
//! `scan format`.
//!
//! Junto de cada mapa vai a lista das peças da busca, cada uma ligada ou ainda
//! não ligada ([`crate::io::search_pieces`]): um número só se entende sabendo o
//! que a busca tinha ligado quando ele saiu.
//!
//! O commit, o sujo e o resumo não se adivinham de dentro da régua: vêm das
//! variáveis que o comando de medida (`mustard-rt run measure`) põe ao rodar o
//! programa que ele mesmo compilou. Sem elas a régua recusa, porque rodá-la
//! direto no cargo deixaria a prova em branco.
//!
//! A prova diz também a versão do gancho que as sessões do usuário rodam: o
//! commit que o `mustard-rt` do plugin instalado carimbou em si, ou que não há
//! plugin. O comando de medida o lê do plugin e o põe numa variável; assim o
//! relato mostra se o que foi medido é o que as sessões rodam.
//!
//! Cada mapa é refeito a cada medida ([`rebuild_map`]): o banco velho sai e o
//! `scan` compilado com o mesmo código grava um novo, com a história de cada
//! declaração lida do git até o fim antes de a medida começar (nas sessões
//! ela chega em segundo plano), de modo que nenhuma medida lê o mapa de uma
//! compilação que ficou na máquina nem um mapa com a história pela metade.
//! Se outra leitura da história do mesmo mapa já está em andamento, a medida
//! espera ela terminar e diz na tela que está lendo; e a linha do mapa
//! refeito diz quantos arquivos o git não deixou ler ([`Rebuilt::line`]), para
//! o número nunca esconder um mapa com parte da história faltando. A régua roda
//! noutro processo e não vê essa conta: o comando de medida a leva numa
//! variável ([`UNREAD_VAR`]), e o resultado da régua a traz em cada mapa que
//! ela abriu ([`MapProof::unread`]), na linha `PECAS` e no JSON da prova.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::domain::scan::{Scan, ScanReport};
use crate::io::project_map::{self, BLOCKS};
use crate::io::search_pieces::{self, Piece};
use crate::io::sha256::Sha256;
use crate::platform::error::{Error, Result};

/// O commit medido, em hexadecimal curto, posto pelo comando de medida.
pub const COMMIT_VAR: &str = "MUSTARD_MEASURE_COMMIT";
/// `1` quando havia código por comitar, `0` quando a pasta estava limpa.
pub const DIRTY_VAR: &str = "MUSTARD_MEASURE_DIRTY";
/// O resumo do que estava por comitar; ausente ou vazio quando a pasta estava limpa.
pub const DIFF_VAR: &str = "MUSTARD_MEASURE_DIFF";
/// O arquivo de resultado que o comando de medida pediu (`--out`); vence a
/// variável própria de cada régua.
pub const OUT_VAR: &str = "MUSTARD_MEASURE_OUT";
/// A versão do gancho que as sessões do usuário rodam, como a prova a mostra:
/// o commit do `mustard-rt` do plugin instalado, ou uma das duas frases
/// ([`HOOK_NOT_INSTALLED`], [`HOOK_WITHOUT_COMMIT`]).
pub const HOOK_VAR: &str = "MUSTARD_MEASURE_HOOK";
/// Quantos arquivos a leitura da história não leu em cada mapa que o comando de
/// medida refez: um objeto JSON do caminho do mapa (o banco) para a conta, `{}`
/// quando ele não refez mapa nenhum. A régua a lê para pôr no resultado a conta
/// de cada mapa que abriu.
pub const UNREAD_VAR: &str = "MUSTARD_MEASURE_UNREAD";
/// O que a prova diz do gancho quando não há `mustard-rt` de plugin instalado.
pub const HOOK_NOT_INSTALLED: &str = "não instalado";
/// O que a prova diz do gancho quando há plugin, mas a versão dele não traz o
/// commit em que foi compilado.
pub const HOOK_WITHOUT_COMMIT: &str = "sem commit na versão";

/// A frase de quem roda a régua sem o comando de medida.
const REFUSAL: &str = "rode pelo comando de medida: `mustard-rt run measure <teste>` compila o código certo e põe o commit, o sujo, o resumo do que falta comitar, a versão do gancho e quantos arquivos a história não leu em cada mapa; sem eles a prova ficaria em branco";

/// As cinco variáveis de ambiente que o comando de medida põe para a régua
/// ler, na ordem commit, sujo, resumo, gancho, arquivos que a história não
/// leu. O resumo vai vazio quando a pasta está limpa. `unread` traz o mapa
/// (o banco) de cada árvore que o comando refez e quantos arquivos a leitura
/// da história dela não leu; vazio quando ele não refez árvore nenhuma.
#[must_use]
pub fn measure_vars(commit: &str, dirty: bool, diff: &str, hook: &str, unread: &[(PathBuf, usize)]) -> [(&'static str, String); 5] {
    let unread: serde_json::Map<String, Value> = unread.iter().map(|(map, count)| (map.display().to_string(), json!(count))).collect();
    [
        (COMMIT_VAR, commit.to_string()),
        (DIRTY_VAR, if dirty { "1" } else { "0" }.to_string()),
        (DIFF_VAR, diff.to_string()),
        (HOOK_VAR, hook.to_string()),
        (UNREAD_VAR, Value::Object(unread).to_string()),
    ]
}

/// A frase de quantos arquivos a leitura da história não leu, a mesma na linha
/// do mapa refeito e na do mapa que a régua abriu.
fn unread_phrase(unread: usize) -> String {
    format!("arquivos que a história não leu: {unread}")
}

/// O commit que a versão completa de um `mustard-rt` carimba nele mesmo
/// (`<número> (build N, g<commit>[-dirty] <data>)`), com o `-dirty` quando o
/// programa foi compilado com código por comitar. `None` sem o carimbo.
#[must_use]
pub fn built_commit(version: &str) -> Option<String> {
    stamped_commit(version).map(str::to_string)
}

/// A palavra do commit na versão carimbada, com o `-dirty` se ele vier.
fn stamped_commit(version: &str) -> Option<&str> {
    version.split_once(", g")?.1.split_whitespace().next()
}

/// O caminho do resultado da régua: o que o comando de medida pediu em
/// [`OUT_VAR`] ou, sem ele, o da variável `own` da própria régua.
#[must_use]
pub fn result_path(own: &str) -> Option<String> {
    [OUT_VAR, own].iter().filter_map(|name| std::env::var(name).ok()).find(|path| !path.trim().is_empty())
}

/// Um mapa aberto pela régua, a marca que todos os blocos dele traziam, o
/// estado de cada peça da busca nele e quantos arquivos a leitura da história
/// não leu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapProof {
    pub path: String,
    pub mark: String,
    pub pieces: Vec<Piece>,
    /// Quantos arquivos o git não deixou ler na história deste mapa, como o
    /// comando de medida contou ao refazê-lo; `None` quando ele não refez este
    /// mapa e, por isso, não esperou a história dele.
    pub unread: Option<usize>,
}

impl MapProof {
    /// O mapa como o resultado da régua o guarda: o caminho, a marca, o estado
    /// de cada peça da busca e quantos arquivos a história não leu (`null`
    /// quando o comando de medida não refez o mapa).
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "path": self.path,
            "mark": self.mark,
            "pieces": self.pieces.iter().map(Piece::to_json).collect::<Vec<_>>(),
            "unread": self.unread,
        })
    }

    /// A linha `PECAS` com o estado de cada peça deste mapa e, quando o
    /// comando de medida o refez, quantos arquivos a história não leu.
    #[must_use]
    pub fn pieces_line(&self) -> String {
        let line = search_pieces::line(&self.path, &self.pieces);
        match self.unread {
            Some(unread) => format!("{line}; {}", unread_phrase(unread)),
            None => line,
        }
    }
}

/// A prova de versão de uma medida.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeasureProof {
    /// O commit medido.
    pub commit: String,
    /// Havia código por comitar (arquivo rastreado mudado ou arquivo novo).
    pub dirty: bool,
    /// O resumo do que estava por comitar; vazio quando a pasta estava limpa.
    pub diff: String,
    /// O SHA-256 do programa que rodou, em hexadecimal.
    pub binary_sha256: String,
    /// O caminho do programa que rodou.
    pub binary_path: String,
    /// A versão do gancho que as sessões do usuário rodam: o commit do
    /// `mustard-rt` do plugin instalado, ou o motivo de não haver.
    pub hook: String,
    /// Cada mapa que a régua abriu, na ordem em que o abriu.
    pub maps: Vec<MapProof>,
    /// Quantos arquivos a leitura da história não leu em cada mapa que o
    /// comando de medida refez, pelo caminho do mapa ([`UNREAD_VAR`]).
    pub unread: BTreeMap<String, usize>,
}

impl MeasureProof {
    /// A prova do programa que está rodando: o commit, o sujo e o resumo das
    /// variáveis do ambiente ([`measure_vars`]) e o SHA-256 do próprio
    /// executável.
    ///
    /// # Errors
    /// Sem as variáveis, ou com um executável que não se acha ou não se lê.
    pub fn for_current_exe() -> Result<Self> {
        let exe = std::env::current_exe().map_err(|e| Error::check_failed(format!("o programa de medida não se acha: {e}")))?;
        Self::from_vars(&|name| std::env::var(name).ok(), &exe)
    }

    /// A prova do programa `exe`, com as variáveis lidas por `vars`.
    ///
    /// # Errors
    /// [`REFUSAL`] se faltar o commit, o sujo, o gancho ou a conta dos arquivos
    /// que a história não leu, ou se um sujo vier sem resumo ou a conta não
    /// for lida; erro se o executável não se lê.
    pub fn from_vars(vars: &dyn Fn(&str) -> Option<String>, exe: &Path) -> Result<Self> {
        let given = |name: &str| vars(name).map(|value| value.trim().to_string()).filter(|value| !value.is_empty());
        let commit = given(COMMIT_VAR).ok_or_else(|| Error::check_failed(REFUSAL))?;
        let dirty = match given(DIRTY_VAR).as_deref() {
            Some("1" | "true") => true,
            Some("0" | "false") => false,
            Some(other) => return Err(Error::check_failed(format!("{DIRTY_VAR} diz `{other}`, e só vale 1 ou 0"))),
            None => return Err(Error::check_failed(REFUSAL)),
        };
        let diff = given(DIFF_VAR).unwrap_or_default();
        if dirty && diff.is_empty() {
            return Err(Error::check_failed(format!("{DIRTY_VAR} diz que havia código por comitar e {DIFF_VAR} não traz o resumo dele")));
        }
        let hook = given(HOOK_VAR).ok_or_else(|| Error::check_failed(REFUSAL))?;
        let unread = given(UNREAD_VAR).ok_or_else(|| Error::check_failed(REFUSAL))?;
        let unread = serde_json::from_str::<BTreeMap<String, usize>>(&unread).map_err(|e| {
            Error::check_failed(format!("{UNREAD_VAR} não diz, por mapa, quantos arquivos a história não leu (um objeto JSON do caminho para a conta): {e}"))
        })?;
        let binary_sha256 = sha256_of_file(exe)
            .map_err(|e| Error::check_failed(format!("o programa {} não se lê para o SHA-256: {e}", exe.display())))?;
        Ok(Self {
            commit,
            dirty,
            diff,
            binary_sha256,
            binary_path: exe.display().to_string(),
            hook,
            maps: Vec::new(),
            unread,
        })
    }

    /// A prova como o resultado da régua a guarda no campo `proof`.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "commit": self.commit,
            "dirty": self.dirty,
            "diff": self.diff,
            "binary_sha256": self.binary_sha256,
            "binary_path": self.binary_path,
            "hook": self.hook,
            "maps": self.maps.iter().map(MapProof::to_json).collect::<Vec<_>>(),
        })
    }

    /// A linha que a régua imprime ao fim: `PROVA commit=.. sujo=.. diff=..
    /// sha=.. mapas=.. gancho=..`.
    #[must_use]
    pub fn line(&self) -> String {
        format!(
            "PROVA commit={} sujo={} diff={} sha={} mapas={} gancho={}",
            self.commit,
            if self.dirty { "sim" } else { "nao" },
            if self.diff.is_empty() { "-" } else { &self.diff },
            &self.binary_sha256[..self.binary_sha256.len().min(12)],
            self.maps.len(),
            self.hook,
        )
    }
}

impl MeasureProof {
    /// O que a régua imprime ao fim: a linha `PROVA` e, depois dela, uma linha
    /// `PECAS` por mapa aberto, com o estado de cada peça da busca nele.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![self.line()];
        lines.extend(self.maps.iter().map(MapProof::pieces_line));
        lines
    }
}

/// O SHA-256 do conteúdo de `path`, lido em pedaços.
fn sha256_of_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut chunk = vec![0u8; 1 << 20];
    loop {
        let read = file.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        hasher.update(&chunk[..read]);
    }
    Ok(hasher.hex_digest())
}

/// O mapa refeito de uma árvore: o relato do scan e quantos arquivos a leitura
/// da história não leu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rebuilt {
    /// O relato da passada do scan que gravou o mapa.
    pub scan: ScanReport,
    /// Quantos arquivos o git não deixou ler na história: o mapa segue sem a
    /// lista deles, e a medida diz quantos são.
    pub unread: usize,
}

impl Rebuilt {
    /// A linha que a medida imprime por árvore refeita, com a árvore e quantos
    /// arquivos a leitura da história não leu (zero quando leu todos).
    #[must_use]
    pub fn line(&self, tree: &Path) -> String {
        format!("mapa refeito: {}; {}", tree.display(), unread_phrase(self.unread))
    }
}

/// O que uma recusa diz, sem o rótulo do tipo dela: o que embrulha o erro de
/// dentro põe o rótulo uma vez, na frente de tudo.
fn reason(error: &Error) -> String {
    match error {
        Error::CheckFailed(reason) => reason.clone(),
        other => other.to_string(),
    }
}

/// Refaz o mapa da árvore `tree` do zero, dizendo o que faz na saída de erro:
/// [`rebuild_map_telling`] com a saída de erro como `tell`.
///
/// # Errors
/// Os de [`rebuild_map_telling`].
pub fn rebuild_map(tree: &Path, scan: &Scan) -> Result<Rebuilt> {
    rebuild_map_telling(tree, scan, &|line| eprintln!("{line}"))
}

/// Refaz o mapa da árvore `tree` do zero: apaga o banco velho e o que o
/// SQLite deixa ao lado dele ([`project_map::remove`]) e roda, sobre a árvore,
/// os mesmos dois passos que o mapa das sessões roda: o scan e, depois dele, a
/// leitura da história de cada declaração. Nas sessões a história chega em
/// segundo plano; aqui a medida espera ela terminar ([`Scan::read_history`]),
/// porque a régua que busca com a história pela metade dá um número diferente
/// a cada rodada. Se uma sessão aberta ou outro comando já está lendo a
/// história do mesmo mapa, a medida espera essa leitura acabar antes de pedir
/// a sua, e antes de esperar diz por `tell` que está lendo a história daquela
/// árvore, porque a leitura leva minutos. Assim o mapa que a régua
/// abre é sempre o que o `scan` compilado com o código medido grava, nunca o
/// de uma compilação que ficou na pasta, e traz a história inteira que a busca
/// das sessões também lê, menos os arquivos que o git não deixou ler, contados
/// em [`Rebuilt::unread`].
///
/// # Errors
/// O arquivo velho que não se apaga; o scan que não roda ou falha; a leitura
/// da história que falha ou que outra leitura do mesmo mapa, começada depois
/// da espera, impede: sem mapa novo e completo, a medida não segue.
pub fn rebuild_map_telling(tree: &Path, scan: &Scan, tell: &dyn Fn(&str)) -> Result<Rebuilt> {
    let model = project_map::model_path(tree);
    project_map::remove(&model)
        .map_err(|e| Error::check_failed(format!("não consegui apagar o mapa velho de {}: {e}", tree.display())))?;
    let report = scan
        .scan(tree, &model)
        .map_err(|e| Error::check_failed(format!("o scan não refez o mapa de {}: {}", tree.display(), reason(&e))))?;
    tell(&format!(
        "lendo a história do mapa de {}; se outra leitura dele já estiver em andamento, espero ela terminar antes",
        tree.display()
    ));
    let unread = scan
        .read_history(tree, &model)
        .map_err(|e| Error::check_failed(format!("o scan não leu a história do mapa de {}: {}", tree.display(), reason(&e))))?;
    Ok(Rebuilt { scan: report, unread })
}

/// Confere que todo bloco do mapa em `db` traz a marca `expected`, a que o
/// `scan` compilado com o código medido diz.
///
/// # Errors
/// O mapa que falta ou não se abre; um bloco sem marca; um bloco de marca
/// diferente (`marca do mapa X, o código compilado produz Y`). Um bloco só
/// que destoe já recusa o mapa inteiro.
pub fn check_map(db: &Path, expected: &str) -> Result<MapProof> {
    if expected.trim().is_empty() {
        return Err(Error::check_failed("o scan compilado não disse a marca esperada: não há com que conferir o mapa"));
    }
    let marks = project_map::read_marks_at(db)
        .map_err(|refusal| Error::check_failed(format!("o mapa {} não abriu ({})", db.display(), refusal.reason())))?;
    for block in &BLOCKS {
        let name = block.name();
        match marks.get(name).map(String::as_str) {
            Some(mark) if mark == expected => {}
            Some(mark) if !mark.is_empty() => {
                return Err(Error::check_failed(format!(
                    "marca do mapa {mark}, o código compilado produz {expected} (bloco {name}, mapa {})",
                    db.display()
                )));
            }
            _ => {
                return Err(Error::check_failed(format!(
                    "o bloco {name} do mapa {} não tem marca; o código compilado produz {expected}",
                    db.display()
                )));
            }
        }
    }
    let pieces = search_pieces::of_map(db)?;
    Ok(MapProof { path: db.display().to_string(), mark: expected.to_string(), pieces, unread: None })
}

/// O que a compilação do programa de medida carimbou nele mesmo: a versão
/// completa (`<número> (build N, g<commit>[-dirty] <data>)`) e o resumo do
/// que estava por comitar. Só o `mustard-rt` tem esse carimbo.
#[derive(Debug, Clone, Copy)]
pub struct BuiltStamp<'a> {
    pub version: &'a str,
    pub diff: &'a str,
}

impl BuiltStamp<'_> {
    /// O commit e o sujo que a versão carimbada diz; `None` sem o bloco do git.
    fn commit_and_dirty(&self) -> Option<(&str, bool)> {
        let word = stamped_commit(self.version)?;
        Some(word.strip_suffix("-dirty").map_or((word, false), |commit| (commit, true)))
    }
}

/// Confere que o programa foi compilado do código que a medida diz medir.
fn check_built(proof: &MeasureProof, built: &BuiltStamp<'_>) -> Result<()> {
    let Some((commit, dirty)) = built.commit_and_dirty() else {
        return Err(Error::check_failed(format!(
            "o programa de medida não traz o commit em que foi compilado (versão `{}`)",
            built.version
        )));
    };
    let same_commit = proof.commit.starts_with(commit) || commit.starts_with(&proof.commit);
    if !same_commit || dirty != proof.dirty || built.diff != proof.diff {
        let state = |dirty: bool, diff: &str| if dirty { format!("sujo ({diff})") } else { "limpo".to_string() };
        return Err(Error::check_failed(format!(
            "o programa foi compilado no commit {commit}, {}; a medida diz {}, {}",
            state(dirty, built.diff),
            proof.commit,
            state(proof.dirty, &proof.diff)
        )));
    }
    Ok(())
}

/// A porta comum das réguas: junta a prova do programa com a marca que o scan
/// compilado espera e confere cada mapa antes de a régua abri-lo.
#[derive(Debug)]
pub struct MeasureGate {
    expected: String,
    proof: MeasureProof,
}

impl MeasureGate {
    /// Abre a porta para o programa que está rodando: a prova do ambiente e a
    /// marca que o `scan` compilado ao lado dele diz (`scan format`). Com
    /// `built`, confere também que o programa foi compilado do commit, do
    /// sujo e do resumo que a medida diz.
    ///
    /// # Errors
    /// A prova sem variáveis; o scan compilado que não está ao lado do
    /// programa (o que a busca no `PATH` acharia pode ser outra versão) ou
    /// que não diz a marca; o programa compilado de outro código.
    pub fn open(built: Option<&BuiltStamp<'_>>) -> Result<Self> {
        let proof = MeasureProof::for_current_exe()?;
        let scan = Scan::locate();
        if !scan.is_compiled_alongside() {
            return Err(Error::check_failed(
                "o scan compilado com este código não está ao lado do programa de medida; o do PATH pode ser de outra versão. Rode pelo comando de medida",
            ));
        }
        let expected = scan
            .format()
            .ok_or_else(|| Error::check_failed("o scan compilado não respondeu `scan format`, e sem a marca não há como conferir o mapa"))?;
        Self::with(proof, expected, built)
    }

    /// A porta com a prova e a marca esperada dadas.
    ///
    /// # Errors
    /// O programa compilado de outro código que o da prova (com `built`).
    pub fn with(proof: MeasureProof, expected: String, built: Option<&BuiltStamp<'_>>) -> Result<Self> {
        if let Some(built) = built {
            check_built(&proof, built)?;
        }
        Ok(Self { expected, proof })
    }

    /// Confere o mapa em `db` e o põe na prova, com quantos arquivos a
    /// história dele não leu quando o comando de medida o refez. A régua o
    /// chama antes de abrir o mapa para medir.
    ///
    /// # Errors
    /// Os de [`check_map`].
    pub fn check(&mut self, db: &Path) -> Result<()> {
        let mut map = check_map(db, &self.expected)?;
        map.unread = self.proof.unread.get(&map.path).copied();
        self.proof.maps.push(map);
        Ok(())
    }

    /// A prova até aqui: com um mapa por `check` que passou.
    #[must_use]
    pub fn proof(&self) -> &MeasureProof {
        &self.proof
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::normalize::Languages;
    use crate::io::project_map::{FILES, model_path, save_at, save_block_at};
    use tempfile::{TempDir, tempdir};

    #[cfg(unix)]
    use crate::domain::scan::tests::{logged, script_scan};
    #[cfg(unix)]
    use crate::io::fs::lock::LockedFile;
    #[cfg(unix)]
    use crate::io::map_lineage::reading_lock_path;

    /// Um mapa pequeno gravado como o scan grava, com `mark` em cada bloco.
    fn saved_with(mark: &str) -> (TempDir, std::path::PathBuf) {
        let dir = tempdir().unwrap();
        let model = model_path(dir.path());
        let map = json!({"modules": [{"path": "src/a.rs", "declarations": [
            {"kind": "function", "name": "a", "line": 1, "end_line": 3, "signature": "fn a()"}]}]});
        save_at(&model, &map, mark, &Languages::of_project(dir.path())).unwrap();
        (dir, model)
    }

    /// Um executável de mentira com o conteúdo `abc`, de SHA-256 conhecido.
    fn exe_abc(dir: &Path) -> std::path::PathBuf {
        let exe = dir.join("programa");
        std::fs::write(&exe, b"abc").unwrap();
        exe
    }

    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn vars(commit: &str, dirty: &str, diff: &str) -> impl Fn(&str) -> Option<String> {
        vars_with_hook(commit, dirty, diff, "10d66039a5b1")
    }

    fn vars_with_hook(commit: &str, dirty: &str, diff: &str, hook: &str) -> impl Fn(&str) -> Option<String> {
        vars_with_unread(commit, dirty, diff, hook, "{}")
    }

    /// As variáveis do comando de medida com a conta `unread` de arquivos que a
    /// história não leu, como o texto que a variável leva.
    fn vars_with_unread(commit: &str, dirty: &str, diff: &str, hook: &str, unread: &str) -> impl Fn(&str) -> Option<String> {
        let all = [
            (COMMIT_VAR, commit.to_string()),
            (DIRTY_VAR, dirty.to_string()),
            (DIFF_VAR, diff.to_string()),
            (HOOK_VAR, hook.to_string()),
            (UNREAD_VAR, unread.to_string()),
        ];
        move |name| all.iter().find(|(key, _)| *key == name).map(|(_, value)| value.clone())
    }

    #[test]
    fn a_map_with_the_expected_mark_passes_and_is_recorded_by_path() {
        let (_dir, model) = saved_with("scan 1");
        let proof = check_map(&model, "scan 1").unwrap();
        assert_eq!((proof.path.as_str(), proof.mark.as_str()), (model.display().to_string().as_str(), "scan 1"));
        let names: Vec<&str> = proof.pieces.iter().map(|piece| piece.name).collect();
        assert_eq!(
            names,
            ["compilado", "raiz-e-sinonimos", "sentido-pelo-vetor", "duas-linguas", "historico", "conferencia"],
            "the map is recorded with the state of every piece of the search"
        );
    }

    /// O fim da régua imprime a linha da prova e uma linha de peças por mapa
    /// aberto, cada uma com o caminho do mapa e o estado de cada peça.
    #[test]
    fn the_ruler_prints_the_proof_line_and_one_pieces_line_per_map() {
        let dir = tempdir().unwrap();
        let proof = MeasureProof::from_vars(&vars("0123456789ab", "0", ""), &exe_abc(dir.path())).unwrap();
        let (_one, first) = saved_with("scan 1");
        let (_two, second) = saved_with("scan 1");
        let mut gate = MeasureGate::with(proof, "scan 1".to_string(), None).unwrap();
        gate.check(&first).unwrap();
        gate.check(&second).unwrap();
        let lines = gate.proof().lines();
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(lines[0].starts_with("PROVA commit=0123456789ab"), "{lines:?}");
        for (line, model) in lines[1..].iter().zip([&first, &second]) {
            assert!(line.starts_with(&format!("PECAS {}: compilado=ainda-nao-ligada", model.display())), "{line}");
            assert!(line.ends_with("conferencia=ligada"), "{line}");
        }
    }

    #[test]
    fn a_map_of_another_mark_is_refused_with_both_marks_in_the_message() {
        let (_dir, model) = saved_with("scan 1");
        let error = check_map(&model, "scan 2").unwrap_err().to_string();
        assert!(error.contains("marca do mapa scan 1, o código compilado produz scan 2"), "{error}");
    }

    #[test]
    fn one_block_of_another_mark_refuses_the_whole_map() {
        let (dir, model) = saved_with("scan 1");
        let map = json!({"modules": [{"path": "src/a.rs", "declarations": []}]});
        assert!(save_block_at(&model, &FILES, &map, "scan 2").unwrap(), "the block was rewritten");
        let error = check_map(&model, "scan 1").unwrap_err().to_string();
        assert!(error.contains("marca do mapa scan 2, o código compilado produz scan 1"), "{error}");
        assert!(error.contains("bloco files"), "{error}");
        drop(dir);
    }

    #[test]
    fn a_map_with_empty_marks_is_refused() {
        let (_dir, model) = saved_with("");
        let error = check_map(&model, "scan 1").unwrap_err().to_string();
        assert!(error.contains("não tem marca"), "{error}");
        assert!(check_map(&model, "").is_err(), "no expected mark, nothing to compare with");
    }

    #[test]
    fn a_missing_map_is_refused() {
        let dir = tempdir().unwrap();
        let error = check_map(&model_path(dir.path()), "scan 1").unwrap_err().to_string();
        assert!(error.contains("não abriu"), "{error}");
    }

    #[test]
    fn the_proof_carries_commit_dirty_diff_binary_hash_and_one_mark_per_opened_map() {
        let dir = tempdir().unwrap();
        let exe = exe_abc(dir.path());
        let proof = MeasureProof::from_vars(&vars("0123456789ab", "1", "feedc0ffee12"), &exe).unwrap();
        assert_eq!(proof.commit, "0123456789ab");
        assert!(proof.dirty);
        assert_eq!(proof.diff, "feedc0ffee12");
        assert_eq!(proof.binary_sha256, ABC, "the hash of the file, not of its name");
        assert_eq!(proof.binary_path, exe.display().to_string());

        let (_one, first) = saved_with("scan 1");
        let (_two, second) = saved_with("scan 1");
        let mut gate = MeasureGate::with(proof, "scan 1".to_string(), None).unwrap();
        gate.check(&first).unwrap();
        gate.check(&second).unwrap();
        let json = gate.proof().to_json();
        assert_eq!(json["maps"].as_array().unwrap().len(), 2, "one entry per opened map: {json}");
        assert_eq!(json["maps"][0]["path"], first.display().to_string());
        assert_eq!(json["maps"][1]["mark"], "scan 1");
        assert_eq!(json["maps"][0]["pieces"].as_array().unwrap().len(), 6, "the pieces go in the result: {json}");
        assert_eq!(json["maps"][0]["pieces"][5]["name"], "conferencia");
        assert_eq!(json["maps"][0]["pieces"][0]["state"], "ainda não ligada", "a map without vectors has no compiled text: {json}");
        assert_eq!(json["dirty"], true);
        assert_eq!(json["binary_sha256"], ABC);
        let line = gate.proof().line();
        assert_eq!(line, format!("PROVA commit=0123456789ab sujo=sim diff=feedc0ffee12 sha={} mapas=2 gancho=10d66039a5b1", &ABC[..12]));
    }

    #[test]
    fn a_clean_tree_is_recorded_clean_with_an_empty_diff() {
        let dir = tempdir().unwrap();
        let proof = MeasureProof::from_vars(&vars("0123456789ab", "0", ""), &exe_abc(dir.path())).unwrap();
        assert!(!proof.dirty);
        assert_eq!(proof.diff, "");
        assert_eq!(proof.line(), format!("PROVA commit=0123456789ab sujo=nao diff=- sha={} mapas=0 gancho=10d66039a5b1", &ABC[..12]));
    }

    #[test]
    fn the_variables_the_measure_command_sets_are_what_the_proof_reads() {
        let dir = tempdir().unwrap();
        let exe = exe_abc(dir.path());
        for (dirty, diff) in [(true, "aaaabbbbcccc"), (false, "")] {
            let set = measure_vars("deadbeef1234", dirty, diff, "10d66039a5b1", &[(PathBuf::from("/a/.claude/grain.db"), 0), (PathBuf::from("/b/.claude/grain.db"), 7)]);
            let read = |name: &str| set.iter().find(|(key, _)| *key == name).map(|(_, value)| value.clone());
            let proof = MeasureProof::from_vars(&read, &exe).unwrap();
            assert_eq!((proof.commit.as_str(), proof.dirty, proof.diff.as_str()), ("deadbeef1234", dirty, diff));
            assert_eq!(proof.hook, "10d66039a5b1");
            assert_eq!(proof.unread, BTreeMap::from([("/a/.claude/grain.db".to_string(), 0), ("/b/.claude/grain.db".to_string(), 7)]));
        }
        let none = measure_vars("deadbeef1234", false, "", "10d66039a5b1", &[]);
        let read = |name: &str| none.iter().find(|(key, _)| *key == name).map(|(_, value)| value.clone());
        assert!(MeasureProof::from_vars(&read, &exe).unwrap().unread.is_empty(), "a measure that rebuilt no map says so");
    }

    /// O resultado da régua diz, em cada mapa que ela abriu, quantos arquivos a
    /// história não leu, como o comando de medida contou ao refazê-lo: o JSON
    /// traz o número, e a linha das peças do mapa o diz depois das peças. O mapa
    /// que o comando não refez sai sem número (`null`) e sem a frase.
    #[test]
    fn the_result_says_how_many_files_the_history_of_each_opened_map_did_not_read() {
        let dir = tempdir().unwrap();
        let (_one, rebuilt) = saved_with("scan 1");
        let (_two, untouched) = saved_with("scan 1");
        let unread = format!("{{{}:2}}", json!(rebuilt.display().to_string()));
        let proof = MeasureProof::from_vars(&vars_with_unread("0123456789ab", "0", "", "10d66039a5b1", &unread), &exe_abc(dir.path())).unwrap();
        let mut gate = MeasureGate::with(proof, "scan 1".to_string(), None).unwrap();
        gate.check(&rebuilt).unwrap();
        gate.check(&untouched).unwrap();

        let json = gate.proof().to_json();
        assert_eq!(json["maps"][0]["unread"], json!(2), "{json}");
        assert!(json["maps"][1]["unread"].is_null(), "a map the command did not rebuild has no count: {json}");
        let lines = gate.proof().lines();
        assert!(lines[1].ends_with("conferencia=ligada; arquivos que a história não leu: 2"), "{lines:?}");
        assert!(lines[2].ends_with("conferencia=ligada"), "{lines:?}");
    }

    /// Sem a conta dos arquivos que a história não leu, ou com ela que não se
    /// lê, a prova recusa: ficaria em branco o que o número esconde do mapa.
    #[test]
    fn a_proof_without_the_count_of_unread_files_or_with_a_count_that_does_not_read_is_refused() {
        let dir = tempdir().unwrap();
        let exe = exe_abc(dir.path());
        let no_count = |name: &str| match name {
            COMMIT_VAR => Some("0123456789ab".to_string()),
            DIRTY_VAR => Some("0".to_string()),
            HOOK_VAR => Some("10d66039a5b1".to_string()),
            _ => None,
        };
        let error = MeasureProof::from_vars(&no_count, &exe).unwrap_err().to_string();
        assert!(error.contains("rode pelo comando de medida") && error.contains("a história não leu"), "{error}");
        for bad in ["[]", "{\"/m\":\"dois\"}", "{\"/m\":-1}", "não é json", "3"] {
            let refused = MeasureProof::from_vars(&vars_with_unread("0123456789ab", "0", "", "10d66039a5b1", bad), &exe).unwrap_err().to_string();
            assert!(refused.contains(UNREAD_VAR), "{bad}: {refused}");
        }
    }

    /// A linha de prova diz o gancho que as sessões do usuário rodam, e o
    /// resultado da régua o leva em `hook`: o commit do plugin ou o motivo de
    /// não haver plugin.
    #[test]
    fn the_proof_line_and_the_result_say_which_hook_the_sessions_run() {
        let dir = tempdir().unwrap();
        let exe = exe_abc(dir.path());
        for hook in ["10d66039a5b1", "10d66039a5b1-dirty", HOOK_NOT_INSTALLED, HOOK_WITHOUT_COMMIT] {
            let proof = MeasureProof::from_vars(&vars_with_hook("0123456789ab", "0", "", hook), &exe).unwrap();
            assert!(proof.line().ends_with(&format!(" gancho={hook}")), "{}", proof.line());
            assert_eq!(proof.to_json()["hook"], hook);
        }
    }

    /// Sem a variável do gancho a prova fica em branco: a régua recusa e
    /// manda rodar pelo comando de medida, como quando falta o commit.
    #[test]
    fn a_proof_without_the_hook_variable_is_refused() {
        let dir = tempdir().unwrap();
        let exe = exe_abc(dir.path());
        let no_hook = |name: &str| match name {
            COMMIT_VAR => Some("0123456789ab".to_string()),
            DIRTY_VAR => Some("0".to_string()),
            _ => None,
        };
        let error = MeasureProof::from_vars(&no_hook, &exe).unwrap_err().to_string();
        assert!(error.contains("rode pelo comando de medida") && error.contains("gancho"), "{error}");
        let blank = vars_with_hook("0123456789ab", "0", "", "  ");
        assert!(MeasureProof::from_vars(&blank, &exe).is_err(), "a blank hook says nothing");
    }

    /// O commit sai da versão completa que o programa carimba, com o `-dirty`
    /// quando o código tinha o que comitar; sem o carimbo, nada.
    #[test]
    fn the_commit_of_a_program_is_read_from_its_stamped_version() {
        assert_eq!(built_commit("mustard-rt 0.2.4 (build dev, g10d66039a5b1 2026-09-25)").as_deref(), Some("10d66039a5b1"));
        assert_eq!(built_commit("0.2.4 (build 7, gabc123456789-dirty 2026-09-30)").as_deref(), Some("abc123456789-dirty"));
        assert_eq!(built_commit("mustard-rt 0.2.4"), None);
        assert_eq!(built_commit(""), None);
    }

    /// Escreve na pasta `.claude` da árvore o banco velho e o que o SQLite
    /// deixa ao lado dele.
    #[cfg(unix)]
    fn stale_map(tree: &Path) -> Vec<std::path::PathBuf> {
        use crate::io::project_map::{MAP_FILE_NAME, MAP_JOURNAL_FILE_NAME, MAP_SHARED_FILE_NAME, MAP_WAL_FILE_NAME};
        let model = model_path(tree);
        std::fs::create_dir_all(model.parent().unwrap()).unwrap();
        [MAP_FILE_NAME, MAP_JOURNAL_FILE_NAME, MAP_WAL_FILE_NAME, MAP_SHARED_FILE_NAME]
            .iter()
            .map(|name| {
                let file = model.with_file_name(name);
                std::fs::write(&file, "de outra compilação").unwrap();
                file
            })
            .collect()
    }

    /// O banco velho e os arquivos que o SQLite deixa ao lado dele saem antes
    /// do scan, e o scan roda sobre a árvore: nenhum mapa velho sobrevive.
    #[cfg(unix)]
    #[test]
    fn rebuilding_a_map_deletes_the_old_database_and_its_side_files_before_the_scan_runs() {
        let tree = tempdir().unwrap();
        let old = stale_map(tree.path());
        let beside = tree.path().join(".claude").join("outro-arquivo");
        std::fs::write(&beside, "não é do mapa").unwrap();

        rebuild_map(tree.path(), &Scan::new("true")).expect("a scan that exits clean rebuilds");

        for file in &old {
            assert!(!file.exists(), "{} sobreviveu", file.display());
        }
        assert!(beside.exists(), "only the map files go");
    }

    /// O scan que falha recusa: o erro diz a árvore, e o banco velho já saiu,
    /// para a régua nunca abri-lo no lugar do que não veio.
    #[cfg(unix)]
    #[test]
    fn a_scan_that_fails_refuses_and_the_old_map_is_gone() {
        let tree = tempdir().unwrap();
        let old = stale_map(tree.path());

        let error = rebuild_map(tree.path(), &Scan::new("false")).unwrap_err().to_string();

        assert!(error.contains("o scan não refez o mapa") && error.contains(&tree.path().display().to_string()), "{error}");
        assert!(old.iter().all(|file| !file.exists()));
        assert_eq!(error.matches("check failed").count(), 1, "the label of the kind of error shows once: {error}");
        let missing = rebuild_map(tree.path(), &Scan::new("/nao/existe/scan")).unwrap_err().to_string();
        assert!(missing.contains("o scan não refez o mapa"), "{missing}");
    }

    /// Um scan de mentira que anota cada passo em `log`: o scan grava um
    /// banco novo e responde o relato de uma linha; a leitura da história
    /// roda `history`, depois de anotar o pedido.
    #[cfg(unix)]
    fn recording_scan(dir: &Path, log: &Path, history: &str) -> Scan {
        script_scan(
            dir,
            "recording",
            &format!(
                "echo \"$1\" >> '{log}'\n\
                 if [ \"$1\" = scan ]; then mkdir -p \"$(dirname \"$4\")\" && printf novo > \"$4\"; echo '{{\"full\":true,\"read\":[],\"files\":1}}'; fi\n\
                 if [ \"$1\" = history-all ]; then {history}; fi",
                log = log.display()
            ),
        )
    }

    /// A medida só começa com a história lida até o fim: o mapa refeito volta
    /// depois do scan e da leitura inteira dela, nunca com a leitura ainda
    /// rodando, e a conta do registro, feita logo na volta, já a encontra.
    #[cfg(unix)]
    #[test]
    fn a_rebuilt_map_comes_back_only_after_the_history_was_read_to_the_end() {
        let (work, tree) = (tempdir().unwrap(), tempdir().unwrap());
        let log = work.path().join("log");
        let scan = recording_scan(work.path(), &log, &format!("sleep 1; echo history-finished >> '{}'", log.display()));

        let rebuilt = rebuild_map(tree.path(), &scan).expect("the scan and the reading pass");

        assert!(rebuilt.scan.full, "{rebuilt:?}");
        assert_eq!(logged(&log), ["scan", "history-all", "history-finished"], "the call came back before the reading ended");
        assert_eq!(std::fs::read_to_string(model_path(tree.path())).unwrap(), "novo");
    }

    /// A leitura da história que falha recusa a medida, dizendo a árvore e o
    /// que o scan disse; a que sai sem ler nada porque outra leitura do mesmo
    /// mapa estava em andamento também recusa, e a que passou deixa passar.
    #[cfg(unix)]
    #[test]
    fn a_history_that_fails_or_cannot_be_read_refuses_the_rebuild() {
        let (work, tree) = (tempdir().unwrap(), tempdir().unwrap());
        let log = work.path().join("log");
        let tree_name = tree.path().display().to_string();

        let fails = recording_scan(work.path(), &log, "echo 'git quebrou' >&2; exit 3");
        let error = rebuild_map(tree.path(), &fails).unwrap_err().to_string();
        assert!(error.contains("o scan não leu a história do mapa") && error.contains(&tree_name) && error.contains("git quebrou"), "{error}");
        assert_eq!(error.matches("check failed").count(), 1, "the label of the kind of error shows once: {error}");
        assert_eq!(logged(&log), ["scan", "history-all"], "the scan ran first and the reading was asked");

        let busy = recording_scan(work.path(), &log, "echo '{\"ok\":true,\"busy\":true,\"files\":0}'");
        let error = rebuild_map(tree.path(), &busy).unwrap_err().to_string();
        assert!(error.contains("o scan não leu a história do mapa") && error.contains("outra leitura") && error.contains("rode de novo"), "{error}");
        assert_eq!(error.matches("check failed").count(), 1, "the label of the kind of error shows once: {error}");

        let reads = recording_scan(work.path(), &log, "echo '{\"ok\":true,\"busy\":false,\"files\":1}'");
        rebuild_map(tree.path(), &reads).expect("a reading that read passes");
    }

    /// Outra leitura da história do mesmo mapa segura a trava por um segundo
    /// (a de uma sessão aberta): a medida diz na tela que está lendo a história
    /// daquela árvore antes de esperar, não pede a leitura enquanto a outra a
    /// segura, e termina com o mapa refeito quando ela solta.
    #[cfg(unix)]
    #[test]
    fn a_rebuild_waits_for_another_reading_of_the_history_and_says_it_is_reading_before_it_waits() {
        let (work, tree) = (tempdir().unwrap(), tempdir().unwrap());
        let log = work.path().join("log");
        let scan = recording_scan(work.path(), &log, "echo '{\"ok\":true,\"busy\":false,\"files\":1}'");
        let other_reading = LockedFile::exclusive(&reading_lock_path(&model_path(tree.path()))).unwrap();
        let told = std::sync::Mutex::new(Vec::<String>::new());

        let (said, asked, finished, outcome) = std::thread::scope(|scope| {
            let rebuilding = scope.spawn(|| rebuild_map_telling(tree.path(), &scan, &|line| told.lock().unwrap().push(line.to_string())));
            let limit = std::time::Instant::now() + std::time::Duration::from_secs(60);
            while told.lock().unwrap().is_empty() && std::time::Instant::now() < limit {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
            let (said, asked, finished) = (told.lock().unwrap().clone(), logged(&log), rebuilding.is_finished());
            drop(other_reading);
            (said, asked, finished, rebuilding.join().unwrap())
        });

        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].contains(&format!("lendo a história do mapa de {}", tree.path().display())) && said[0].contains("espero ela terminar"), "{said:?}");
        assert_eq!(asked, ["scan"], "the history was asked while the other reading still held the map");
        assert!(!finished, "the rebuild came back before the other reading let go");
        let rebuilt = outcome.expect("the rebuild passes once the other reading let go");
        assert_eq!(rebuilt.unread, 0);
        assert_eq!(logged(&log), ["scan", "history-all"]);
    }

    /// O git que não lê a história de alguns arquivos deixa a medida seguir, e
    /// a linha do mapa refeito diz quantos: o número do relato do scan em
    /// `failed`, com zero quando ele leu todos.
    #[cfg(unix)]
    #[test]
    fn the_rebuilt_map_line_says_how_many_files_the_history_did_not_read() {
        let (work, tree) = (tempdir().unwrap(), tempdir().unwrap());
        let log = work.path().join("log");
        let name = tree.path().display().to_string();

        let partial = recording_scan(work.path(), &log, "echo '{\"ok\":true,\"busy\":false,\"files\":5,\"failed\":2}'");
        let rebuilt = rebuild_map(tree.path(), &partial).expect("a reading with files left out still rebuilds");
        assert_eq!(rebuilt.unread, 2);
        assert_eq!(rebuilt.line(tree.path()), format!("mapa refeito: {name}; arquivos que a história não leu: 2"));

        let whole = recording_scan(work.path(), &log, "echo '{\"ok\":true,\"busy\":false,\"files\":5,\"failed\":0}'");
        let rebuilt = rebuild_map(tree.path(), &whole).unwrap();
        assert_eq!(rebuilt.line(tree.path()), format!("mapa refeito: {name}; arquivos que a história não leu: 0"));
    }

    #[test]
    fn without_the_variables_the_proof_refuses_and_says_to_run_by_the_measure_command() {
        let dir = tempdir().unwrap();
        let exe = exe_abc(dir.path());
        let none = |_: &str| None;
        let error = MeasureProof::from_vars(&none, &exe).unwrap_err().to_string();
        assert!(error.contains("rode pelo comando de medida"), "{error}");
        let only_commit = |name: &str| (name == COMMIT_VAR).then(|| "0123456789ab".to_string());
        let error = MeasureProof::from_vars(&only_commit, &exe).unwrap_err().to_string();
        assert!(error.contains("rode pelo comando de medida"), "a proof without the dirty flag stays blank: {error}");
        let error = MeasureProof::from_vars(&vars("0123456789ab", "1", ""), &exe).unwrap_err().to_string();
        assert!(error.contains(DIFF_VAR), "dirty without a diff summary is refused: {error}");
    }

    #[test]
    fn the_program_must_have_been_built_from_the_code_the_measurement_claims() {
        let dir = tempdir().unwrap();
        let proof = MeasureProof::from_vars(&vars("0123456789ab", "1", "feedc0ffee12"), &exe_abc(dir.path())).unwrap();
        let built = |version, diff| BuiltStamp { version, diff };
        let ok = built("0.2.4 (build dev, g0123456789ab-dirty 2026-09-30)", "feedc0ffee12");
        assert!(MeasureGate::with(proof.clone(), "m".into(), Some(&ok)).is_ok());

        let other_commit = built("0.2.4 (build dev, gfffffffffff0-dirty 2026-09-30)", "feedc0ffee12");
        let other_diff = built("0.2.4 (build dev, g0123456789ab-dirty 2026-09-30)", "000000000000");
        let clean = built("0.2.4 (build dev, g0123456789ab 2026-09-30)", "");
        let unstamped = built("0.2.4", "");
        for stamp in [&other_commit, &other_diff, &clean, &unstamped] {
            let error = MeasureGate::with(proof.clone(), "m".into(), Some(stamp)).unwrap_err().to_string();
            assert!(error.contains("compilado") || error.contains("commit"), "{error}");
        }
    }
}
