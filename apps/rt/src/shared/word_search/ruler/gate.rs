//! A porta da régua: a prova de que o número saiu do código que a régua diz
//! medir.
//!
//! Um número de medida só vale com a versão que o gerou. O resultado da régua
//! leva o commit de que o programa foi compilado, se havia código por comitar
//! (e um resumo do que era), o SHA-256 do programa que rodou e a marca de cada
//! mapa que ela abriu. O commit e o resumo vêm do carimbo que a compilação
//! deixou no próprio programa, nunca de variável posta à mão.
//!
//! Um mapa feito por outra compilação do `scan` é recusado antes de qualquer
//! número sair: o banco guarda em cada bloco a marca de quem o encheu, e a
//! marca esperada é a que o `scan` compilado ao lado do programa diz em
//! `scan format`.

use std::io::Read;
use std::path::Path;

use mustard_core::domain::scan::Scan;
use mustard_core::io::project_map::{self, BLOCKS};
use mustard_core::io::sha256::Sha256;
use mustard_core::platform::error::{Error, Result};
use serde_json::{json, Value};

/// O que a compilação deste programa carimbou nele mesmo: a versão completa
/// (`<número> (build N, g<commit>[-dirty] <data>)`) e o resumo do que estava
/// por comitar.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BuiltStamp<'a> {
    pub(crate) version: &'a str,
    pub(crate) diff: &'a str,
}

impl BuiltStamp<'static> {
    /// O carimbo do programa que está rodando.
    pub(crate) fn of_this_program() -> Self {
        Self { version: env!("MUSTARD_VERSION_FULL"), diff: env!("MUSTARD_GIT_DIFF") }
    }
}

impl BuiltStamp<'_> {
    /// O commit e o sujo que a versão carimbada diz; `None` sem o bloco do git.
    fn commit_and_dirty(&self) -> Option<(&str, bool)> {
        let word = self.version.split_once(", g")?.1.split_whitespace().next()?;
        Some(word.strip_suffix("-dirty").map_or((word, false), |commit| (commit, true)))
    }
}

/// Um mapa aberto pela régua e a marca que todos os blocos dele traziam.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MapProof {
    pub(crate) path: String,
    pub(crate) mark: String,
}

/// A prova de versão de uma medida.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MeasureProof {
    /// O commit de que o programa foi compilado.
    pub(crate) commit: String,
    /// Havia código por comitar (arquivo rastreado mudado ou arquivo novo).
    pub(crate) dirty: bool,
    /// O resumo do que estava por comitar; vazio quando a pasta estava limpa.
    pub(crate) diff: String,
    /// O SHA-256 do programa que rodou, em hexadecimal.
    pub(crate) binary_sha256: String,
    /// O caminho do programa que rodou.
    pub(crate) binary_path: String,
    /// Cada mapa que a régua abriu, na ordem em que o abriu.
    pub(crate) maps: Vec<MapProof>,
}

impl MeasureProof {
    /// A prova do programa `exe`, compilado com o carimbo `built`.
    ///
    /// # Errors
    /// O carimbo sem o commit (programa compilado fora do git) ou um
    /// executável que não se lê.
    pub(crate) fn of_program(built: &BuiltStamp<'_>, exe: &Path) -> Result<Self> {
        let Some((commit, dirty)) = built.commit_and_dirty() else {
            return Err(Error::check_failed(format!(
                "o programa da régua não traz o commit em que foi compilado (versão `{}`)",
                built.version
            )));
        };
        let binary_sha256 = sha256_of_file(exe)
            .map_err(|e| Error::check_failed(format!("o programa {} não se lê para o SHA-256: {e}", exe.display())))?;
        Ok(Self {
            commit: commit.to_string(),
            dirty,
            diff: built.diff.to_string(),
            binary_sha256,
            binary_path: exe.display().to_string(),
            maps: Vec::new(),
        })
    }

    /// A prova como o resultado da régua a guarda no campo `proof`.
    pub(crate) fn to_json(&self) -> Value {
        json!({
            "commit": self.commit,
            "dirty": self.dirty,
            "diff": self.diff,
            "binary_sha256": self.binary_sha256,
            "binary_path": self.binary_path,
            "maps": self.maps.iter().map(|map| json!({ "path": map.path, "mark": map.mark })).collect::<Vec<_>>(),
        })
    }

    /// A linha que a régua imprime ao fim: `PROVA commit=.. sujo=.. diff=..
    /// sha=.. mapas=..`.
    pub(crate) fn line(&self) -> String {
        format!(
            "PROVA commit={} sujo={} diff={} sha={} mapas={}",
            self.commit,
            if self.dirty { "sim" } else { "nao" },
            if self.diff.is_empty() { "-" } else { &self.diff },
            &self.binary_sha256[..self.binary_sha256.len().min(12)],
            self.maps.len(),
        )
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

/// Confere que todo bloco do mapa em `db` traz a marca `expected`, a que o
/// `scan` compilado ao lado do programa diz.
///
/// # Errors
/// O mapa que falta ou não se abre; um bloco sem marca; um bloco de marca
/// diferente (`marca do mapa X, o código compilado produz Y`). Um bloco só
/// que destoe já recusa o mapa inteiro.
fn check_map(db: &Path, expected: &str) -> Result<MapProof> {
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
    Ok(MapProof { path: db.display().to_string(), mark: expected.to_string() })
}

/// A porta comum das réguas: junta a prova do programa com a marca que o scan
/// compilado espera e confere cada mapa antes de a régua abri-lo.
#[derive(Debug)]
pub(crate) struct MeasureGate {
    expected: String,
    proof: MeasureProof,
}

impl MeasureGate {
    /// Abre a porta para o programa que está rodando: a prova do carimbo dele
    /// e a marca que o `scan` compilado ao lado dele diz (`scan format`).
    ///
    /// # Errors
    /// O programa sem o commit no carimbo; o scan compilado que não está ao
    /// lado do programa (o que a busca no `PATH` acharia pode ser outra
    /// versão) ou que não diz a marca.
    pub(crate) fn open() -> Result<Self> {
        let exe = std::env::current_exe().map_err(|e| Error::check_failed(format!("o programa da régua não se acha: {e}")))?;
        let proof = MeasureProof::of_program(&BuiltStamp::of_this_program(), &exe)?;
        let scan = Scan::locate();
        if !scan.is_compiled_alongside() {
            return Err(Error::check_failed(
                "o scan compilado com este código não está ao lado do programa da régua; o do PATH pode ser de outra versão. Compile o scan junto (`cargo build --release -p scan`)",
            ));
        }
        let expected = scan
            .format()
            .ok_or_else(|| Error::check_failed("o scan compilado não respondeu `scan format`, e sem a marca não há como conferir o mapa"))?;
        Ok(Self::with(proof, expected))
    }

    /// A porta com a prova e a marca esperada dadas.
    pub(crate) fn with(proof: MeasureProof, expected: String) -> Self {
        Self { expected, proof }
    }

    /// Confere o mapa em `db` e o põe na prova. A régua o chama antes de abrir
    /// o mapa para medir.
    ///
    /// # Errors
    /// Os de [`check_map`].
    pub(crate) fn check(&mut self, db: &Path) -> Result<()> {
        self.proof.maps.push(check_map(db, &self.expected)?);
        Ok(())
    }

    /// A prova até aqui: com um mapa por `check` que passou.
    pub(crate) fn proof(&self) -> &MeasureProof {
        &self.proof
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::normalize::Languages;
    use mustard_core::io::project_map::{model_path, save_at, save_block_at, FILES};
    use tempfile::{tempdir, TempDir};

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

    /// O commit e o sujo saem da versão que o programa carimba, com o `-dirty`
    /// quando o código tinha o que comitar; sem o carimbo do git, nada.
    #[test]
    fn the_commit_and_the_dirty_flag_are_read_from_the_stamped_version() {
        let clean = BuiltStamp { version: "mustard-rt 0.2.4 (build dev, g10d66039a5b1 2026-09-25)", diff: "" };
        assert_eq!(clean.commit_and_dirty(), Some(("10d66039a5b1", false)));
        let dirty = BuiltStamp { version: "0.2.4 (build 7, gabc123456789-dirty 2026-09-30)", diff: "feedc0ffee12" };
        assert_eq!(dirty.commit_and_dirty(), Some(("abc123456789", true)));
        assert_eq!(BuiltStamp { version: "mustard-rt 0.2.4", diff: "" }.commit_and_dirty(), None);
        assert_eq!(BuiltStamp { version: "", diff: "" }.commit_and_dirty(), None);
    }

    /// A prova leva o commit, o sujo e o resumo do carimbo do programa, o hash
    /// do arquivo dele e, depois dos `check`, uma marca por mapa aberto.
    #[test]
    fn the_proof_carries_the_stamp_the_binary_hash_and_one_mark_per_opened_map() {
        let dir = tempdir().unwrap();
        let exe = exe_abc(dir.path());
        let stamp = BuiltStamp { version: "0.2.4 (build dev, g0123456789ab-dirty 2026-09-30)", diff: "feedc0ffee12" };
        let proof = MeasureProof::of_program(&stamp, &exe).unwrap();
        assert_eq!(proof.commit, "0123456789ab");
        assert!(proof.dirty);
        assert_eq!(proof.diff, "feedc0ffee12");
        assert_eq!(proof.binary_sha256, ABC, "the hash of the file, not of its name");
        assert_eq!(proof.binary_path, exe.display().to_string());

        let (_one, first) = saved_with("scan 1");
        let (_two, second) = saved_with("scan 1");
        let mut gate = MeasureGate::with(proof, "scan 1".to_string());
        gate.check(&first).unwrap();
        gate.check(&second).unwrap();
        let json = gate.proof().to_json();
        assert_eq!(json["maps"].as_array().unwrap().len(), 2, "one entry per opened map: {json}");
        assert_eq!(json["maps"][0]["path"], first.display().to_string());
        assert_eq!(json["maps"][1]["mark"], "scan 1");
        assert_eq!(json["dirty"], true);
        assert_eq!(json["binary_sha256"], ABC);
        assert_eq!(gate.proof().line(), format!("PROVA commit=0123456789ab sujo=sim diff=feedc0ffee12 sha={} mapas=2", &ABC[..12]));
    }

    #[test]
    fn a_clean_program_is_recorded_clean_with_an_empty_diff() {
        let dir = tempdir().unwrap();
        let stamp = BuiltStamp { version: "0.2.4 (build dev, g0123456789ab 2026-09-30)", diff: "" };
        let proof = MeasureProof::of_program(&stamp, &exe_abc(dir.path())).unwrap();
        assert!(!proof.dirty);
        assert_eq!(proof.diff, "");
        assert_eq!(proof.line(), format!("PROVA commit=0123456789ab sujo=nao diff=- sha={} mapas=0", &ABC[..12]));
    }

    /// O programa compilado fora do git não traz o commit, e a prova dele ficaria
    /// em branco: a porta recusa.
    #[test]
    fn a_program_without_the_commit_in_its_stamp_gives_no_proof() {
        let dir = tempdir().unwrap();
        let unstamped = BuiltStamp { version: "0.2.4", diff: "" };
        let refused = MeasureProof::of_program(&unstamped, &exe_abc(dir.path())).unwrap_err().to_string();
        assert!(refused.contains("não traz o commit"), "{refused}");
    }

    /// O hash lê o arquivo inteiro, também quando ele passa de um pedaço (o
    /// executável de teste tem dezenas de megabytes).
    #[test]
    fn the_hash_of_a_file_larger_than_one_chunk_covers_all_its_bytes() {
        let dir = tempdir().unwrap();
        let bytes: Vec<u8> = (0..(2 << 20) + 12_345).map(|n: u32| (n % 251) as u8).collect();
        let exe = dir.path().join("grande");
        std::fs::write(&exe, &bytes).unwrap();
        let mut whole = Sha256::new();
        whole.update(&bytes);
        assert_eq!(sha256_of_file(&exe).unwrap(), whole.hex_digest());
    }

    #[test]
    fn a_map_with_the_expected_mark_passes_and_is_recorded_by_path() {
        let (_dir, model) = saved_with("scan 1");
        let proof = check_map(&model, "scan 1").unwrap();
        assert_eq!((proof.path.as_str(), proof.mark.as_str()), (model.display().to_string().as_str(), "scan 1"));
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
}
