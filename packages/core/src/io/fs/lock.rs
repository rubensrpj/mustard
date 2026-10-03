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
        let file = open_creating(path)?;
        file.lock()?;
        Ok(Self { file })
    }

    /// Como [`LockedFile::exclusive`], mas sem esperar: `None` quando outro
    /// manipulador segura a trava agora.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] quando a pasta não pode ser criada, o arquivo não abre ou
    /// a trava falha por outro motivo que não estar presa.
    pub fn exclusive_if_free(path: &Path) -> Result<Option<Self>> {
        let file = open_creating(path)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Self { file })),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(std::fs::TryLockError::Error(e)) => Err(e.into()),
        }
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
    /// disco confirmou. Quem chama passa a linha sem o `\n`. Se a escrita ou a
    /// confirmação falha, o arquivo volta ao tamanho de antes, sem sobra de
    /// linha pela metade.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] quando a escrita falha.
    pub fn append_line(&mut self, line: &str) -> Result<()> {
        Ok(append_or_rollback(&mut self.file, line)?)
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

/// O que a gravação de uma linha pede do destino: escrever, medir o fim,
/// cortar até um tamanho e confirmar no disco. O arquivo de verdade cumpre
/// tudo; o teste põe no lugar um destino que enche de propósito.
trait Appendable: Write + Seek {
    /// Corta o destino em `len` bytes.
    fn shrink_to(&mut self, len: u64) -> std::io::Result<()>;

    /// Espera o disco confirmar o que foi escrito.
    fn sync(&mut self) -> std::io::Result<()>;
}

impl Appendable for File {
    fn shrink_to(&mut self, len: u64) -> std::io::Result<()> {
        self.set_len(len)
    }

    fn sync(&mut self) -> std::io::Result<()> {
        self.sync_data()
    }
}

/// Acrescenta `line` e um `\n` ao fim de `target`, num só bloco, e deixa o
/// arquivo como estava se a escrita ou a confirmação falha: o disco cheio no
/// meio da linha não deixa meia linha para trás. O erro que volta é o da
/// escrita; se o corte também falha, ele não o esconde.
fn append_or_rollback<T: Appendable>(target: &mut T, line: &str) -> std::io::Result<()> {
    let start = target.seek(SeekFrom::End(0))?;
    let mut bytes = Vec::with_capacity(line.len() + 1);
    bytes.extend_from_slice(line.as_bytes());
    bytes.push(b'\n');
    let written = target.write_all(&bytes).and_then(|()| target.sync());
    if let Err(err) = written {
        let _ = target.shrink_to(start);
        return Err(err);
    }
    Ok(())
}

/// Abre `path` para ler e escrever, criando o arquivo e a pasta quando
/// faltam, sem mexer no conteúdo.
fn open_creating(path: &Path) -> Result<File> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    Ok(OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path)?)
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

    /// A trava pedida sem esperar volta vazia enquanto outro manipulador a
    /// segura, sem esperar por ele, e volta presa depois que ele solta.
    #[test]
    fn the_lock_asked_without_waiting_is_empty_while_another_handle_holds_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/round.lock");
        let held = LockedFile::exclusive(&path).unwrap();
        assert!(LockedFile::exclusive_if_free(&path).unwrap().is_none(), "held elsewhere: nothing");
        drop(held);
        let free = LockedFile::exclusive_if_free(&path).unwrap();
        assert!(free.is_some(), "free: the lock is taken");
        assert!(LockedFile::exclusive_if_free(&path).unwrap().is_none(), "and now it is held by the first");
    }

    /// Um destino que aceita `room` bytes e então diz que o disco encheu, como
    /// o disco cheio de verdade no meio de uma linha. Guarda o tamanho de cada
    /// escrita para provar que a linha sai num bloco só.
    struct Full {
        data: Vec<u8>,
        pos: usize,
        room: usize,
        writes: Vec<usize>,
        sync_fails: bool,
        shrink_fails: bool,
    }

    impl Full {
        fn new(data: &str, room: usize) -> Self {
            Self { data: data.as_bytes().to_vec(), pos: 0, room, writes: Vec::new(), sync_fails: false, shrink_fails: false }
        }

        fn text(&self) -> String {
            String::from_utf8(self.data.clone()).unwrap()
        }
    }

    impl Write for Full {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if self.room == 0 {
                return Err(std::io::Error::from(ErrorKind::StorageFull));
            }
            let taken = buf.len().min(self.room);
            self.room -= taken;
            self.writes.push(taken);
            self.data.truncate(self.pos);
            self.data.extend_from_slice(&buf[..taken]);
            self.pos += taken;
            Ok(taken)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Seek for Full {
        fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
            self.pos = match to {
                SeekFrom::Start(n) => n as usize,
                SeekFrom::End(n) => self.data.len().saturating_add_signed(n as isize),
                SeekFrom::Current(n) => self.pos.saturating_add_signed(n as isize),
            };
            Ok(self.pos as u64)
        }
    }

    impl Appendable for Full {
        fn shrink_to(&mut self, len: u64) -> std::io::Result<()> {
            if self.shrink_fails {
                return Err(std::io::Error::from(ErrorKind::PermissionDenied));
            }
            self.data.truncate(len as usize);
            Ok(())
        }

        fn sync(&mut self) -> std::io::Result<()> {
            if self.sync_fails {
                return Err(std::io::Error::from(ErrorKind::StorageFull));
            }
            Ok(())
        }
    }

    /// O disco enche 5 bytes depois de a linha começar: o que já tinha 8 bytes
    /// termina com 8 bytes, o mesmo conteúdo, e o erro é o do disco cheio.
    #[test]
    fn a_line_cut_by_a_full_disk_leaves_the_file_as_it_was() {
        let mut target = Full::new("um\ndois\n", 5);
        let err = append_or_rollback(&mut target, "tres-quatro").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::StorageFull);
        assert_eq!(target.writes, [5], "five bytes went in before the disk filled");
        assert_eq!(target.data.len(), 8);
        assert_eq!(target.text(), "um\ndois\n");
    }

    /// A confirmação do disco que falha, com a linha inteira já escrita, também
    /// desfaz a linha: quem recebe o erro não fica com a gravação pela metade.
    #[test]
    fn a_failed_confirmation_takes_the_written_line_back() {
        let mut target = Full::new("um\n", 100);
        target.sync_fails = true;
        let err = append_or_rollback(&mut target, "dois").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::StorageFull);
        assert_eq!(target.text(), "um\n");
    }

    /// Se o corte também falha, o erro que volta continua sendo o da escrita.
    #[test]
    fn a_failed_cut_does_not_hide_the_write_error() {
        let mut target = Full::new("um\n", 2);
        target.shrink_fails = true;
        let err = append_or_rollback(&mut target, "dois").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::StorageFull, "not the cut's PermissionDenied");
    }

    /// A linha e o `\n` saem numa escrita só, com o tamanho da linha mais um.
    #[test]
    fn the_line_and_its_newline_go_out_in_one_write() {
        let mut target = Full::new("", 100);
        append_or_rollback(&mut target, "abcdef").unwrap();
        assert_eq!(target.writes, [7]);
        assert_eq!(target.text(), "abcdef\n");
    }

    /// No arquivo de verdade que só devolve `StorageFull` (o `/dev/full`), a
    /// gravação pela trava responde o erro do disco, mesmo com o corte
    /// impossível nele.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_locked_append_to_a_full_device_answers_the_disk_error() {
        let mut file = LockedFile::exclusive(Path::new("/dev/full")).unwrap();
        match file.append_line("um") {
            Err(Error::Io(e)) => assert_eq!(e.kind(), ErrorKind::StorageFull),
            other => panic!("expected the disk error, got {other:?}"),
        }
    }

    /// Uma gravação pela trava que esbarra no limite de tamanho de arquivo do
    /// processo (o `ulimit -f`) grava os 40 bytes que cabem e falha: o arquivo
    /// tem que terminar com o tamanho e o conteúdo de antes. O limite vale para
    /// o processo inteiro, então o teste roda a si mesmo num filho com ele.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_locked_append_cut_by_the_size_limit_leaves_the_file_as_it_was() {
        const FILE: &str = "MUSTARD_APPEND_LIMIT_FILE";
        if let Ok(path) = std::env::var(FILE) {
            let mut file = LockedFile::exclusive(Path::new(&path)).unwrap();
            let err = file.append_line(&"x".repeat(100)).unwrap_err();
            assert!(matches!(err, Error::Io(_)), "{err:?}");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        // 64 KiB de limite; o arquivo já tem 40 bytes a menos que isso.
        let before = format!("{}\n", "a".repeat(64 * 1024 - 41));
        std::fs::write(&path, &before).unwrap();
        let test = "a_locked_append_cut_by_the_size_limit_leaves_the_file_as_it_was";
        let out = std::process::Command::new("bash")
            .args(["-c", "trap '' XFSZ; ulimit -f 64 && exec \"$0\" \"$@\""])
            .arg(std::env::current_exe().unwrap())
            .args([test, "--test-threads=1"])
            .env(FILE, &path)
            .output()
            .expect("bash runs the test binary under the size limit");
        assert!(out.status.success(), "the child failed: {}", String::from_utf8_lossy(&out.stdout));
        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(after.len(), 64 * 1024 - 40, "same size as before");
        assert_eq!(after, before, "same content as before");
    }
}
