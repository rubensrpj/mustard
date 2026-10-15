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
use std::hash::{Hash, Hasher};
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::platform::error::{Error, Result};

/// O arquivo de trava do trabalho `kind` sobre `target`: na pasta temporária,
/// com o nome tirado do caminho inteiro de `target`, para não deixar arquivo
/// no projeto. O mesmo trabalho sobre o mesmo alvo dá sempre o mesmo arquivo.
#[must_use]
pub fn temp_lock_path(kind: &str, target: &Path) -> PathBuf {
    let whole = std::path::absolute(target).unwrap_or_else(|_| target.to_path_buf());
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    whole.hash(&mut hasher);
    std::env::temp_dir().join(format!("mustard-{kind}-{:016x}.lock", hasher.finish()))
}

/// Um arquivo aberto com a trava exclusiva. A trava solta quando o valor sai
/// de cena.
#[derive(Debug)]
pub struct LockedFile {
    file: File,
    /// O caminho travado: a troca de conteúdo põe o arquivo novo nele.
    path: PathBuf,
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
        loop {
            let file = open_creating(path)?;
            file.lock()?;
            if still_at(&file, path)? {
                return Ok(Self { file, path: path.to_path_buf() });
            }
        }
    }

    /// Como [`LockedFile::exclusive`], mas sem esperar: `None` quando outro
    /// manipulador segura a trava agora.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] quando a pasta não pode ser criada, o arquivo não abre ou
    /// a trava falha por outro motivo que não estar presa.
    pub fn exclusive_if_free(path: &Path) -> Result<Option<Self>> {
        loop {
            let file = open_creating(path)?;
            match file.try_lock() {
                Ok(()) if still_at(&file, path)? => return Ok(Some(Self { file, path: path.to_path_buf() })),
                Ok(()) => {}
                Err(std::fs::TryLockError::WouldBlock) => return Ok(None),
                Err(std::fs::TryLockError::Error(e)) => return Err(e.into()),
            }
        }
    }

    /// Como [`LockedFile::exclusive_if_free`], mas espera a trava soltar por
    /// até `patience`: `None` só quando outro manipulador ainda a segura no
    /// fim da espera. Serve a quem grava algo curto e prefere esperar a
    /// gravação do lado terminar a perder a própria.
    ///
    /// # Errors
    ///
    /// Os mesmos de [`LockedFile::exclusive_if_free`].
    pub fn exclusive_within(path: &Path, patience: std::time::Duration) -> Result<Option<Self>> {
        let deadline = std::time::Instant::now() + patience;
        loop {
            if let Some(held) = Self::exclusive_if_free(path)? {
                return Ok(Some(held));
            }
            let now = std::time::Instant::now();
            if now >= deadline {
                return Ok(None);
            }
            std::thread::sleep(LOCK_POLL.min(deadline - now));
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
        loop {
            let file = match OpenOptions::new().read(true).write(true).open(path) {
                Ok(file) => file,
                Err(e) if e.kind() == ErrorKind::NotFound => {
                    return Err(Error::NotFound(path.display().to_string()));
                }
                Err(e) => return Err(e.into()),
            };
            file.lock()?;
            if still_at(&file, path)? {
                return Ok(Self { file, path: path.to_path_buf() });
            }
        }
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

    /// Troca o conteúdo inteiro por `contents`. Onde o sistema diz qual
    /// arquivo mora num caminho (Unix), o conteúdo novo vai inteiro para um
    /// arquivo ao lado, já com a trava, e só então toma o lugar do velho: o
    /// processo morto no meio da escrita deixa o arquivo de antes intacto, e
    /// quem esperava a trava no arquivo velho percebe a troca e trava o novo.
    /// Nos outros sistemas a troca regrava o mesmo arquivo, e se a escrita, o
    /// corte ou a confirmação falha, o conteúdo de antes volta.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] quando a escrita falha; o erro é o da escrita, mesmo
    /// quando a volta do conteúdo de antes também falha.
    pub fn replace(&mut self, contents: &[u8]) -> Result<()> {
        if cfg!(unix) {
            self.file = swap_in(&self.file, &self.path, contents)?;
            return Ok(());
        }
        self.file.seek(SeekFrom::Start(0))?;
        let mut before = Vec::new();
        self.file.read_to_end(&mut before)?;
        let file = &mut self.file;
        let written = file
            .seek(SeekFrom::Start(0))
            .and_then(|_| file.write_all(contents))
            .and_then(|()| file.set_len(contents.len() as u64))
            .and_then(|()| if super::disk_sync() { file.sync_all() } else { Ok(()) });
        if let Err(err) = written {
            // As páginas do conteúdo de antes já estavam no disco: regravá-las
            // não pede espaço novo.
            let _ = file.seek(SeekFrom::Start(0)).and_then(|_| file.write_all(&before)).and_then(|()| file.set_len(before.len() as u64));
            return Err(err.into());
        }
        Ok(())
    }
}

impl Drop for LockedFile {
    fn drop(&mut self) {
        // Fechar o arquivo já solta a trava; soltar antes deixa claro quando.
        let _ = self.file.unlock();
    }
}

/// O intervalo entre as tentativas de quem espera a trava por tempo limitado.
const LOCK_POLL: std::time::Duration = std::time::Duration::from_millis(10);

/// Se `file`, já travado, ainda é o arquivo que mora em `path`. A troca de
/// conteúdo põe um arquivo novo no caminho; quem esperava a trava no velho
/// recebe `false` e abre o caminho de novo, em vez de ler e gravar num
/// arquivo que ninguém mais lê.
#[cfg(unix)]
fn still_at(file: &File, path: &Path) -> Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let held = file.metadata()?;
    match std::fs::metadata(path) {
        Ok(now) => Ok(held.dev() == now.dev() && held.ino() == now.ino()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// Fora do Unix a troca regrava o mesmo arquivo, então o arquivo travado é
/// sempre o do caminho.
#[cfg(not(unix))]
fn still_at(_file: &File, _path: &Path) -> Result<bool> {
    Ok(true)
}

/// Grava `contents` num arquivo novo ao lado de `path`, com as permissões de
/// `old`, confirma no disco, trava o novo e só então o põe no lugar de
/// `path`. Volta o arquivo novo, já travado: entre a troca e a volta ninguém
/// o pega. Se algo falha antes da troca, o arquivo ao lado some e `path`
/// fica como estava.
fn swap_in(old: &File, path: &Path, contents: &[u8]) -> std::io::Result<File> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let side = path.with_file_name(format!(".{name}.{}-{serial}.swap", std::process::id()));
    let swapped = (|| {
        let mut file = OpenOptions::new().read(true).write(true).create_new(true).open(&side)?;
        file.set_permissions(old.metadata()?.permissions())?;
        file.lock()?;
        file.write_all(contents)?;
        if super::disk_sync() {
            file.sync_all()?;
        }
        std::fs::rename(&side, path)?;
        Ok(file)
    })();
    if swapped.is_err() {
        let _ = std::fs::remove_file(&side);
    }
    swapped
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
        if super::disk_sync() {
            self.sync_data()?;
        }
        Ok(())
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
    let mut file = loop {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == ErrorKind::NotFound => {
                return Err(Error::NotFound(path.display().to_string()));
            }
            Err(e) => return Err(e.into()),
        };
        file.lock_shared()?;
        if still_at(&file, path)? {
            break file;
        }
    };
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

    /// Uma troca de conteúdo que esbarra no limite de tamanho de arquivo do
    /// processo (o `ulimit -f`, o mesmo efeito do disco cheio) falha e deixa o
    /// arquivo com o conteúdo de antes, que continua se lendo inteiro.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_locked_replace_cut_by_the_size_limit_keeps_the_previous_content() {
        const FILE: &str = "MUSTARD_REPLACE_LIMIT_FILE";
        if let Ok(path) = std::env::var(FILE) {
            let mut file = LockedFile::exclusive(Path::new(&path)).unwrap();
            let err = file.replace(format!("{{\"events\":\"{}\"}}", "b".repeat(80 * 1024)).as_bytes()).unwrap_err();
            assert!(matches!(err, Error::Io(_)), "{err:?}");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("diagnostics.json");
        let before = format!("{{\"events\":\"{}\"}}", "a".repeat(40 * 1024));
        std::fs::write(&path, &before).unwrap();
        let test = "a_locked_replace_cut_by_the_size_limit_keeps_the_previous_content";
        let out = std::process::Command::new("bash")
            .args(["-c", "trap '' XFSZ; ulimit -f 64 && exec \"$0\" \"$@\""])
            .arg(std::env::current_exe().unwrap())
            .args([test, "--test-threads=1"])
            .env(FILE, &path)
            .output()
            .expect("bash runs the test binary under the size limit");
        assert!(out.status.success(), "the child failed: {}", String::from_utf8_lossy(&out.stdout));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before, "same content as before");
    }

    /// O processo morto pelo limite de tamanho de arquivo (o `ulimit -f` sem
    /// ignorar o sinal dele) no meio de uma troca de conteúdo deixa o arquivo
    /// com o conteúdo de antes, inteiro: a escrita cortada é a de um arquivo
    /// ao lado, que nunca tomou o lugar do outro.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_process_killed_in_the_middle_of_a_replace_leaves_the_previous_content() {
        use std::os::unix::process::ExitStatusExt;
        const FILE: &str = "MUSTARD_REPLACE_KILLED_FILE";
        if let Ok(path) = std::env::var(FILE) {
            let mut file = LockedFile::exclusive(Path::new(&path)).unwrap();
            let _ = file.replace(format!("{{\"events\":\"{}\"}}", "b".repeat(80 * 1024)).as_bytes());
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("diagnostics.json");
        let before = format!("{{\"events\":\"{}\"}}", "a".repeat(40 * 1024));
        std::fs::write(&path, &before).unwrap();
        let test = "a_process_killed_in_the_middle_of_a_replace_leaves_the_previous_content";
        let out = std::process::Command::new("bash")
            .args(["-c", "ulimit -f 64 && exec \"$0\" \"$@\""])
            .arg(std::env::current_exe().unwrap())
            .args([test, "--test-threads=1"])
            .env(FILE, &path)
            .output()
            .expect("bash runs the test binary under the size limit");
        assert_eq!(out.status.signal(), Some(25), "the child dies by SIGXFSZ mid-write: {:?}", out.status);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before, "same content as before");
    }

    /// Quem esperava a trava enquanto o conteúdo foi trocado trava o arquivo
    /// que ficou no caminho: lê o conteúdo novo, e a linha que acrescenta fica
    /// no arquivo que todos leem.
    #[test]
    fn a_waiter_during_a_replace_locks_the_file_left_at_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.json");
        let mut held = LockedFile::exclusive(&path).unwrap();
        held.replace(b"um\n").unwrap();
        let waiter = {
            let path = path.clone();
            std::thread::spawn(move || {
                let mut file = LockedFile::exclusive(&path).unwrap();
                let seen = file.read_to_string().unwrap();
                file.append_line("tres").unwrap();
                seen
            })
        };
        std::thread::sleep(std::time::Duration::from_millis(200));
        held.replace(b"dois\n").unwrap();
        drop(held);
        assert_eq!(waiter.join().unwrap(), "dois\n");
        assert_eq!(read_shared(&path).unwrap(), "dois\ntres\n");
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
