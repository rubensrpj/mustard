//! Travas de arquivo: a gravação que precisa ler, decidir e escrever sem que
//! outro processo grave no meio.
//!
//! A trava é a `File::lock` da biblioteca padrão, que funciona igual no Linux,
//! no macOS e no Windows. No Windows a trava é obrigatória e vale por
//! manipulador: enquanto um manipulador segura a trava, nenhum outro — nem do
//! mesmo processo — lê ou escreve o arquivo. Por isso quem trava lê e escreve
//! só pelo próprio [`LockedFile`], nunca pelas funções soltas de [`super`], e
//! quem só lê usa [`read_shared`], que espera a trava de escrita soltar.
//!
//! Como [`super::real`], este módulo fala com o disco de verdade: uma trava só
//! existe num arquivo real, e não há dublê de teste para ela.

use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::platform::error::{Error, Result};

/// Um arquivo aberto com a trava exclusiva. A trava solta quando o valor sai
/// de cena.
#[derive(Debug)]
pub struct LockedFile {
    file: File,
}

impl LockedFile {
    /// Abre `path` para ler e escrever, criando o arquivo e a pasta quando
    /// faltam, e espera a trava exclusiva.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] quando a pasta não pode ser criada, o arquivo não abre ou
    /// a trava falha.
    pub fn exclusive(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path)?;
        file.lock()?;
        Ok(Self { file })
    }

    /// Abre `path`, que precisa existir, para ler e escrever, e espera a trava
    /// exclusiva. Não cria arquivo nem pasta.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`] quando o arquivo não existe; [`Error::Io`] quando ele
    /// não abre ou a trava falha.
    pub fn existing(path: &Path) -> Result<Self> {
        let file = match OpenOptions::new().read(true).write(true).open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == ErrorKind::NotFound => {
                return Err(Error::NotFound(path.display().to_string()));
            }
            Err(e) => return Err(e.into()),
        };
        file.lock()?;
        Ok(Self { file })
    }

    /// O conteúdo inteiro, lido pelo manipulador que segura a trava. Um byte
    /// que não é UTF-8 vira `U+FFFD`: a linha dele deixa de se entender, e o
    /// resto do arquivo continua legível.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] quando a leitura falha.
    pub fn read_to_string(&mut self) -> Result<String> {
        self.file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        self.file.read_to_end(&mut bytes)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Acrescenta `line` e um `\n` no fim, de uma vez, e só volta depois que o
    /// disco confirmou. Quem chama passa a linha sem o `\n`.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] quando a escrita falha.
    pub fn append_line(&mut self, line: &str) -> Result<()> {
        self.file.seek(SeekFrom::End(0))?;
        let mut bytes = Vec::with_capacity(line.len() + 1);
        bytes.extend_from_slice(line.as_bytes());
        bytes.push(b'\n');
        self.file.write_all(&bytes)?;
        self.file.sync_data()?;
        Ok(())
    }

    /// Troca o conteúdo inteiro por `contents`, pelo mesmo manipulador. Não é
    /// uma troca atômica por arquivo novo: trocar o arquivo de lugar deixaria
    /// quem espera a trava preso ao arquivo velho.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] quando a escrita falha.
    pub fn replace(&mut self, contents: &[u8]) -> Result<()> {
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(contents)?;
        self.file.set_len(contents.len() as u64)?;
        self.file.sync_all()?;
        Ok(())
    }
}

impl Drop for LockedFile {
    fn drop(&mut self) {
        // Fechar o arquivo já solta a trava; soltar antes deixa claro quando.
        let _ = self.file.unlock();
    }
}

/// Lê `path` inteiro com a trava compartilhada: espera uma gravação em curso
/// terminar, então nunca vê uma linha pela metade dela.
///
/// # Errors
///
/// [`Error::NotFound`] quando o arquivo não existe; [`Error::Io`] quando a
/// leitura ou a trava falham.
pub fn read_shared(path: &Path) -> Result<String> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Err(Error::NotFound(path.display().to_string()));
        }
        Err(e) => return Err(e.into()),
    };
    file.lock_shared()?;
    let mut bytes = Vec::new();
    let read = file.read_to_end(&mut bytes);
    let _ = file.unlock();
    read?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_locked_file_reads_and_appends_through_its_own_handle() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/b/log.ndjson");
        let mut file = LockedFile::exclusive(&path).unwrap();
        assert_eq!(file.read_to_string().unwrap(), "");
        file.append_line("um").unwrap();
        file.append_line("dois").unwrap();
        assert_eq!(file.read_to_string().unwrap(), "um\ndois\n");
        file.replace(b"tres\n").unwrap();
        drop(file);
        assert_eq!(read_shared(&path).unwrap(), "tres\n");
    }

    #[test]
    fn reading_a_missing_file_says_not_found() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(read_shared(&dir.path().join("x")), Err(Error::NotFound(_))));
    }

    #[test]
    fn locking_an_existing_file_never_creates_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/log.ndjson");
        assert!(matches!(LockedFile::existing(&path), Err(Error::NotFound(_))));
        assert!(!path.exists() && !path.parent().unwrap().exists(), "nothing was created");
        LockedFile::exclusive(&path).unwrap().append_line("um").unwrap();
        let mut file = LockedFile::existing(&path).unwrap();
        assert_eq!(file.read_to_string().unwrap(), "um\n");
    }
}
