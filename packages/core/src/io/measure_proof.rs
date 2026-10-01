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
//! O commit, o sujo e o resumo não se adivinham de dentro da régua: vêm das
//! variáveis que o comando de medida (`mustard-rt run measure`) põe ao rodar o
//! programa que ele mesmo compilou. Sem elas a régua recusa, porque rodá-la
//! direto no cargo deixaria a prova em branco.

use std::io::Read;
use std::path::Path;

use serde_json::{Value, json};

use crate::domain::scan::Scan;
use crate::io::project_map::{self, BLOCKS};
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

/// A frase de quem roda a régua sem o comando de medida.
const REFUSAL: &str = "rode pelo comando de medida: `mustard-rt run measure <teste>` compila o código certo e põe o commit, o sujo e o resumo do que falta comitar; sem eles a prova ficaria em branco";

/// As três variáveis de ambiente que o comando de medida põe para a régua
/// ler, na ordem commit, sujo, resumo. O resumo vai vazio quando a pasta está
/// limpa.
#[must_use]
pub fn measure_vars(commit: &str, dirty: bool, diff: &str) -> [(&'static str, String); 3] {
    [
        (COMMIT_VAR, commit.to_string()),
        (DIRTY_VAR, if dirty { "1" } else { "0" }.to_string()),
        (DIFF_VAR, diff.to_string()),
    ]
}

/// O caminho do resultado da régua: o que o comando de medida pediu em
/// [`OUT_VAR`] ou, sem ele, o da variável `own` da própria régua.
#[must_use]
pub fn result_path(own: &str) -> Option<String> {
    [OUT_VAR, own].iter().filter_map(|name| std::env::var(name).ok()).find(|path| !path.trim().is_empty())
}

/// Um mapa aberto pela régua e a marca que todos os blocos dele traziam.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapProof {
    pub path: String,
    pub mark: String,
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
    /// Cada mapa que a régua abriu, na ordem em que o abriu.
    pub maps: Vec<MapProof>,
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
    /// [`REFUSAL`] se faltar o commit ou o sujo, ou se um sujo vier sem
    /// resumo; erro se o executável não se lê.
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
        let binary_sha256 = sha256_of_file(exe)
            .map_err(|e| Error::check_failed(format!("o programa {} não se lê para o SHA-256: {e}", exe.display())))?;
        Ok(Self {
            commit,
            dirty,
            diff,
            binary_sha256,
            binary_path: exe.display().to_string(),
            maps: Vec::new(),
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
            "maps": self.maps.iter().map(|map| json!({"path": map.path, "mark": map.mark})).collect::<Vec<_>>(),
        })
    }

    /// A linha que a régua imprime ao fim: `PROVA commit=.. sujo=.. diff=..
    /// sha=.. mapas=..`.
    #[must_use]
    pub fn line(&self) -> String {
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
    Ok(MapProof { path: db.display().to_string(), mark: expected.to_string() })
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
        let after = self.version.split_once(", g")?.1;
        let word = after.split_whitespace().next()?;
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

    /// Confere o mapa em `db` e o põe na prova. A régua o chama antes de
    /// abrir o mapa para medir.
    ///
    /// # Errors
    /// Os de [`check_map`].
    pub fn check(&mut self, db: &Path) -> Result<()> {
        let map = check_map(db, &self.expected)?;
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
        let all = [(COMMIT_VAR, commit.to_string()), (DIRTY_VAR, dirty.to_string()), (DIFF_VAR, diff.to_string())];
        move |name| all.iter().find(|(key, _)| *key == name).map(|(_, value)| value.clone())
    }

    #[test]
    fn a_map_with_the_expected_mark_passes_and_is_recorded_by_path() {
        let (_dir, model) = saved_with("scan 1");
        let proof = check_map(&model, "scan 1").unwrap();
        assert_eq!(proof, MapProof { path: model.display().to_string(), mark: "scan 1".to_string() });
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
        assert_eq!(json["dirty"], true);
        assert_eq!(json["binary_sha256"], ABC);
        let line = gate.proof().line();
        assert_eq!(line, format!("PROVA commit=0123456789ab sujo=sim diff=feedc0ffee12 sha={} mapas=2", &ABC[..12]));
    }

    #[test]
    fn a_clean_tree_is_recorded_clean_with_an_empty_diff() {
        let dir = tempdir().unwrap();
        let proof = MeasureProof::from_vars(&vars("0123456789ab", "0", ""), &exe_abc(dir.path())).unwrap();
        assert!(!proof.dirty);
        assert_eq!(proof.diff, "");
        assert_eq!(proof.line(), format!("PROVA commit=0123456789ab sujo=nao diff=- sha={} mapas=0", &ABC[..12]));
    }

    #[test]
    fn the_variables_the_measure_command_sets_are_what_the_proof_reads() {
        let dir = tempdir().unwrap();
        let exe = exe_abc(dir.path());
        for (dirty, diff) in [(true, "aaaabbbbcccc"), (false, "")] {
            let set = measure_vars("deadbeef1234", dirty, diff);
            let read = |name: &str| set.iter().find(|(key, _)| *key == name).map(|(_, value)| value.clone());
            let proof = MeasureProof::from_vars(&read, &exe).unwrap();
            assert_eq!((proof.commit.as_str(), proof.dirty, proof.diff.as_str()), ("deadbeef1234", dirty, diff));
        }
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
