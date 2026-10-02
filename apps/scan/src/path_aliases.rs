//! Apelidos de pasta dos imports.
//!
//! Algumas línguas deixam o projeto dar nome curto a uma pasta num arquivo de
//! configuração: `@app/pedido` no import quer dizer `src/app/pedido`, e um
//! import não relativo pode ser lido a partir de uma pasta base. Tudo o que é
//! da língua vem do registro (`alias_config`, `alias_base`, `alias_paths` e
//! `alias_extends` em languages.toml): o nome do arquivo e as três chaves lidas
//! nele. Este módulo nunca escreve o nome de uma língua nem o de um arquivo.
//!
//! Cada arquivo que importa lê a configuração mais próxima, subindo as pastas.
//! A língua pode ler mais de um nome de arquivo: na mesma pasta, vale o
//! primeiro nome da lista dela que existe ali, e a pasta mais próxima vence
//! qualquer nome de uma pasta acima. A configuração segue a herança relativa dentro do projeto; a herança de um
//! pacote de fora fica de fora. O arquivo é JSON e aceita comentário e vírgula
//! sobrando. Os arquivos de configuração saem da lista de caminhos que a
//! varredura já guarda, então o que o git ignora não conta.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde_json::Value;

/// As quatro entradas do registro que dizem como uma língua declara apelidos:
/// o nome do arquivo e as chaves da base, dos apelidos e da herança.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Keys {
    file: &'static str,
    base: &'static str,
    paths: &'static str,
    extends: &'static str,
}

impl Keys {
    /// As entradas da língua, uma por nome de arquivo de apelidos, na ordem
    /// de preferência do registro, todas com as mesmas três chaves; vazio
    /// quando ela não tem arquivo de apelidos.
    fn all(lang: &str) -> impl Iterator<Item = Keys> + '_ {
        crate::extract::alias_config(lang).iter().map(move |&file| Keys {
            file,
            base: crate::extract::alias_base(lang),
            paths: crate::extract::alias_paths(lang),
            extends: crate::extract::alias_extends(lang),
        })
    }
}

/// Cada padrão de import e as pastas que ele representa, na ordem escrita.
type Patterns = Vec<(String, Vec<String>)>;

/// Os apelidos que valem para quem lê uma configuração, com a herança já
/// aplicada e toda pasta relativa à raiz do projeto.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Aliases {
    /// A pasta de onde se leem os imports não relativos, quando declarada.
    base: Option<String>,
    /// Cada padrão de import e as pastas que ele representa, na ordem escrita.
    paths: Patterns,
}

impl Aliases {
    /// Os caminhos, relativos à raiz, que um import pode estar citando pelos
    /// apelidos, na ordem em que se tentam: o padrão exato, senão o padrão
    /// com `*` de começo mais longo, e por fim a pasta base. Import relativo
    /// não passa por apelido.
    fn candidates(&self, imp: &str) -> Vec<String> {
        if imp.starts_with('.') || imp.starts_with('/') {
            return Vec::new();
        }
        let mut out: Vec<String> = Vec::new();
        let exact = self.paths.iter().find(|(pattern, _)| !pattern.contains('*') && pattern == imp);
        let matched = exact.map(|(_, targets)| (targets, "")).or_else(|| {
            self.paths
                .iter()
                .filter_map(|(pattern, targets)| {
                    let (head, tail) = pattern.split_once('*')?;
                    let fits = imp.len() >= head.len() + tail.len() && imp.starts_with(head) && imp.ends_with(tail);
                    fits.then(|| (head.len(), targets, &imp[head.len()..imp.len() - tail.len()]))
                })
                .max_by_key(|(head, _, _)| *head)
                .map(|(_, targets, star)| (targets, star))
        });
        if let Some((targets, star)) = matched {
            out.extend(targets.iter().map(|t| t.replacen('*', star, 1)));
        }
        if let Some(base) = &self.base
            && let Some(joined) = join(base, imp)
        {
            out.push(joined);
        }
        out
    }
}

/// O que uma configuração declara, já somado ao que ela herda, antes de os
/// alvos dos apelidos serem lidos a partir da base.
#[derive(Debug, Default, Clone)]
struct Declared {
    /// A pasta base, relativa à raiz.
    base: Option<String>,
    /// Os apelidos como escritos, e a pasta da configuração que os declara.
    paths: Option<(Patterns, String)>,
}

impl Declared {
    /// Esta declaração com a de `child` por cima: o que o filho declara vence.
    fn under(self, child: Declared) -> Declared {
        Declared { base: child.base.or(self.base), paths: child.paths.or(self.paths) }
    }

    /// Os apelidos que valem: os alvos saem da pasta base quando ela existe,
    /// senão da pasta da configuração que declara os apelidos.
    fn resolved(self) -> Aliases {
        let base = self.base;
        let paths = self
            .paths
            .map(|(patterns, dir)| {
                let from = base.clone().unwrap_or(dir);
                patterns
                    .into_iter()
                    .map(|(pattern, targets)| {
                        let targets = targets.iter().filter_map(|t| join(&from, t)).collect();
                        (pattern, targets)
                    })
                    .collect()
            })
            .unwrap_or_default();
        Aliases { base, paths }
    }
}

/// Os apelidos de pasta do projeto: para cada jeito de declarar (as entradas
/// do registro), os apelidos de cada arquivo de configuração achado.
#[derive(Default)]
pub(crate) struct PathAliases {
    configs: HashMap<Keys, HashMap<String, Aliases>>,
}

impl PathAliases {
    /// Lê as configurações de apelidos do projeto em `root`, entre os caminhos
    /// `walk_paths` que a varredura visitou (relativos à raiz, com `/`).
    pub(crate) fn load(root: &Path, walk_paths: &[String]) -> Self {
        let known: HashSet<&str> = walk_paths.iter().map(String::as_str).collect();
        let mut configs: HashMap<Keys, HashMap<String, Aliases>> = HashMap::new();
        // A configuração que duas línguas leem do mesmo jeito se lê uma vez.
        for keys in crate::extract::alias_languages().flat_map(Keys::all) {
            if configs.contains_key(&keys) {
                continue;
            }
            let mut reader = Reader { root, keys, known: &known, done: HashMap::new() };
            let found: HashMap<String, Aliases> = walk_paths
                .iter()
                .filter(|p| file_name(p) == keys.file)
                .map(|p| (p.clone(), reader.declared(p, &mut HashSet::new()).resolved()))
                .collect();
            configs.insert(keys, found);
        }
        PathAliases { configs }
    }

    /// Os caminhos, relativos à raiz, que o import `imp` do arquivo `importer`
    /// (da língua `lang`) pode estar citando pelos apelidos da configuração
    /// mais próxima dele: subindo as pastas, a primeira que tem uma das
    /// configurações da língua, e nela a do primeiro nome da lista. Vazio
    /// quando a língua não tem apelidos, quando nenhuma configuração está
    /// acima do arquivo ou quando nenhum apelido serve.
    pub(crate) fn candidates(&self, importer: &str, lang: &str, imp: &str) -> Vec<String> {
        let found: Vec<(Keys, &HashMap<String, Aliases>)> = Keys::all(lang)
            .filter_map(|keys| self.configs.get(&keys).filter(|found| !found.is_empty()).map(|found| (keys, found)))
            .collect();
        if found.is_empty() {
            return Vec::new();
        }
        let mut dir = parent_dir(importer);
        loop {
            for (keys, found) in &found {
                let config = if dir.is_empty() { keys.file.to_string() } else { format!("{dir}/{}", keys.file) };
                if let Some(aliases) = found.get(&config) {
                    return aliases.candidates(imp);
                }
            }
            if dir.is_empty() {
                return Vec::new();
            }
            dir = parent_dir(&dir);
        }
    }
}

/// Quem lê as configurações de um jeito de declarar, guardando o que cada uma
/// declara com a herança somada, para uma base herdada por muitas ser lida uma
/// vez só.
struct Reader<'a> {
    root: &'a Path,
    keys: Keys,
    known: &'a HashSet<&'a str>,
    done: HashMap<String, Declared>,
}

impl Reader<'_> {
    /// O que a configuração `path` declara, somado ao que ela herda. `visiting`
    /// guarda a cadeia em leitura: uma herança que volta a si mesma para ali.
    fn declared(&mut self, path: &str, visiting: &mut HashSet<String>) -> Declared {
        if let Some(done) = self.done.get(path) {
            return done.clone();
        }
        if !visiting.insert(path.to_string()) {
            return Declared::default();
        }
        let value = std::fs::read_to_string(self.root.join(path))
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&plain_json(&text)).ok())
            .unwrap_or(Value::Null);
        let dir = parent_dir(path);

        let mut declared = Declared::default();
        for parent in self.parents(&value, &dir) {
            let inherited = self.declared(&parent, visiting);
            declared = declared.under(inherited);
        }
        let own = Declared {
            base: lookup(&value, self.keys.base).and_then(Value::as_str).and_then(|b| join(&dir, b)),
            paths: lookup(&value, self.keys.paths).and_then(Value::as_object).map(|object| {
                let patterns = object
                    .iter()
                    .map(|(pattern, targets)| {
                        let targets: Vec<String> = match targets {
                            Value::String(one) => vec![one.clone()],
                            Value::Array(many) => many.iter().filter_map(Value::as_str).map(str::to_string).collect(),
                            _ => Vec::new(),
                        };
                        (pattern.clone(), targets)
                    })
                    .collect();
                (patterns, dir.clone())
            }),
        };
        let declared = declared.under(own);
        visiting.remove(path);
        self.done.insert(path.to_string(), declared.clone());
        declared
    }

    /// As configurações de que `value` (lida na pasta `dir`) herda, na ordem
    /// escrita. Só entra o caminho relativo que cai num arquivo do projeto,
    /// com ou sem a extensão do nome do arquivo de configuração; nome de
    /// pacote e caminho que sai do projeto ficam de fora.
    fn parents(&self, value: &Value, dir: &str) -> Vec<String> {
        let written: Vec<&str> = match lookup(value, self.keys.extends) {
            Some(Value::String(one)) => vec![one.as_str()],
            Some(Value::Array(many)) => many.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        };
        let extension = self.keys.file.rsplit_once('.').map(|(_, ext)| ext);
        written
            .into_iter()
            .filter(|p| p.starts_with("./") || p.starts_with("../"))
            .filter_map(|p| join(dir, p))
            .filter_map(|p| {
                if self.known.contains(p.as_str()) {
                    return Some(p);
                }
                let with_ext = format!("{p}.{}", extension?);
                self.known.contains(with_ext.as_str()).then_some(with_ext)
            })
            .collect()
    }
}

/// O valor no caminho com pontos `key` (`a.b` é o `b` dentro do `a`). `None`
/// para chave vazia ou caminho que não existe.
fn lookup<'v>(value: &'v Value, key: &str) -> Option<&'v Value> {
    if key.is_empty() {
        return None;
    }
    key.split('.').try_fold(value, |at, part| at.get(part))
}

/// O texto JSON de um arquivo que aceita comentário (`//` e `/* */`) e
/// vírgula sobrando antes de `}` ou `]`: os dois saem, e o que está dentro de
/// texto entre aspas fica como está.
fn plain_json(text: &str) -> String {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 1;
            } else if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        match (c, chars.get(i + 1)) {
            ('"', _) => {
                in_string = true;
                out.push(c);
                i += 1;
            }
            ('/', Some('/')) => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            ('/', Some('*')) => {
                i += 2;
                while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                    i += 1;
                }
                i += 2;
                out.push(' ');
            }
            (',', _) => {
                let next = next_meaningful(&chars, i + 1);
                if !matches!(next, Some('}' | ']')) {
                    out.push(c);
                }
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// O primeiro caractere a partir de `from` que não é espaço nem comentário.
fn next_meaningful(chars: &[char], from: usize) -> Option<char> {
    let mut i = from;
    while i < chars.len() {
        match (chars[i], chars.get(i + 1)) {
            (c, _) if c.is_whitespace() => i += 1,
            ('/', Some('/')) => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            ('/', Some('*')) => {
                i += 2;
                while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                    i += 1;
                }
                i += 2;
            }
            (c, _) => return Some(c),
        }
    }
    None
}

/// `rel` lido a partir da pasta `dir` (ambos relativos à raiz), sem `.` nem
/// `..`. `None` quando o caminho sai da raiz do projeto ou é absoluto.
fn join(dir: &str, rel: &str) -> Option<String> {
    if rel.starts_with('/') {
        return None;
    }
    let mut parts: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

/// A pasta de um caminho relativo; vazia na raiz.
fn parent_dir(path: &str) -> String {
    path.rsplit_once('/').map_or(String::new(), |(dir, _)| dir.to_string())
}

/// O último trecho de um caminho.
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_and_trailing_commas_are_dropped_but_not_inside_strings() {
        let text = "\u{feff}{\n  // linha\n  \"a\": \"x//y\", /* bloco */\n  \"b\": [1, 2,],\n  \"c\": \"/*z*/,\",\n}\n";
        let value: Value = serde_json::from_str(&plain_json(text)).expect("vira JSON");
        assert_eq!(value["a"], "x//y");
        assert_eq!(value["b"], serde_json::json!([1, 2]));
        assert_eq!(value["c"], "/*z*/,");
    }

    #[test]
    fn the_exact_pattern_wins_then_the_longest_head_then_the_base() {
        let aliases = Aliases {
            base: Some("src".to_string()),
            paths: vec![
                ("@app/*".to_string(), vec!["src/app/*".to_string()]),
                ("@app/core/*".to_string(), vec!["core/*".to_string()]),
                ("@app/core/x".to_string(), vec!["exato/x".to_string()]),
            ],
        };
        assert_eq!(aliases.candidates("@app/core/y"), vec!["core/y".to_string(), "src/@app/core/y".to_string()]);
        assert_eq!(aliases.candidates("@app/core/x"), vec!["exato/x".to_string(), "src/@app/core/x".to_string()]);
        assert_eq!(aliases.candidates("@app/pedido"), vec!["src/app/pedido".to_string(), "src/@app/pedido".to_string()]);
        assert!(aliases.candidates("./local").is_empty(), "import relativo não passa por apelido");
    }

    #[test]
    fn a_path_leaving_the_project_is_left_out() {
        assert_eq!(join("a/b", "../c"), Some("a/c".to_string()));
        assert_eq!(join("a", "../../c"), None);
        assert_eq!(join("a", "/etc/c"), None);
    }
}
