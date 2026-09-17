//! Texto com cara de segredo: chave, token ou senha.
//!
//! Antes de uma página poder ser publicada, cada trecho dela passa por aqui, e
//! o que casa fica fora da página até o item ser expurgado. Três famílias:
//!
//! - as chaves e os tokens com forma conhecida e o cabeçalho de chave privada,
//!   que casam sozinhos;
//! - a senha, o token ou a chave escritos como atribuição, em qualquer das
//!   formas comuns: `senha: …`, `a senha é …`, `senha do banco: …`,
//!   `DB_PASSWORD=…`, `GITHUB_TOKEN=…`, `"password": "…"`, `client_secret=…`;
//! - o valor que só um segredo tem naquele lugar: a senha dentro de um
//!   endereço (`esquema://usuário:senha@host`, com ou sem o usuário) e o token
//!   depois de `Bearer`.
//!
//! Nas duas últimas, o valor só conta quando parece de verdade: não é um
//! marcador (`<senha>`, `${TOKEN}`, `****`), um código de item, uma data, um
//! caminho com linha ou a leitura de uma variável de ambiente
//! (`process.env.X`), e tem letra e número. A senha num endereço dispensa a
//! letra e o número, porque ali não há outra coisa que não seja senha; fica de
//! fora só a palavra de exemplo no lugar dela (`senha`, `password`, `user`).

use std::sync::OnceLock;

use regex::Regex;

/// As formas de chave e token que não se confundem com texto comum.
const SHAPES: &[&str] = &[
    r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b",
    r"\bgh[pousr]_[A-Za-z0-9]{36,}",
    r"\bgithub_pat_[A-Za-z0-9_]{22,}",
    r"\bglpat-[A-Za-z0-9_-]{20,}",
    r"\b[sr]k_(?:live|test)_[A-Za-z0-9]{24,}",
    r"\bxox[abprs]-[A-Za-z0-9-]{10,}",
    r"\bsk-ant-[A-Za-z0-9_-]{20,}",
    r"\bsk-(?:proj-|svcacct-|admin-)?[A-Za-z0-9_-]{32,}",
    r"\bnpm_[A-Za-z0-9]{36,}",
    r"\bAIza[0-9A-Za-z_-]{35}",
    r"-----BEGIN [A-Z ]*PRIVATE KEY-----",
    r"\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}",
];

/// Os nomes que, antes de `:` ou `=`, anunciam um segredo. O nome pode vir
/// no fim de outra palavra (`DB_PASSWORD`, `GITHUB_TOKEN`, `client_secret`,
/// `SecretKey`, `AccountKey`, `ENCRYPTION_KEY`).
const NAMES: &str = r"password|passwd|pwd|senha|secret|segredo|token|key|chave";

/// As palavras que um exemplo põe no lugar da senha de um endereço.
const EXAMPLE_WORDS: &[&str] = &["senha", "password", "pass", "pwd", "secret", "token", "user", "usuário", "usuario"];

/// As formas em que o valor vem no grupo `v` e só conta se parecer de
/// verdade, cada uma dizendo se o valor precisa de letra e número.
fn valued_patterns() -> [(String, bool); 3] {
    [
        // A atribuição: o nome, com prefixo e com aspa, até duas palavras de
        // ligação (`senha do banco`), o sinal e o valor, com ou sem aspa.
        (
            format!(
                r#"(?i)\b\w*?(?:{NAMES})["']?(?:\s+(?:do|da|de|dos|das|of|for)\s+[\w-]+){{0,2}}\s*(?:[:=]|\sé\s|\sis\s)\s*["']?(?P<v>[^\s"'`<>]{{8,}})"#
            ),
            true,
        ),
        // A senha num endereço: `esquema://usuário:senha@host`, com o usuário
        // podendo faltar (`redis://:senha@host`).
        (r"(?i)\b[a-z][a-z0-9+.-]*://[^\s:/@]*:(?P<v>[^\s:/@]{4,})@".to_string(), false),
        // O token do cabeçalho de autorização.
        (r"(?i)\bbearer\s+(?P<v>[A-Za-z0-9._~+/=-]{16,})".to_string(), true),
    ]
}

/// O começo de um valor que lê uma variável de ambiente em vez de trazer o
/// segredo.
const ENV_READS: &[&str] = &[
    "process.env",
    "import.meta.env",
    "os.environ",
    "os.getenv",
    "std::env",
    "env::var",
    "system.getenv",
    "environment.getenvironmentvariable",
];

fn shapes() -> Option<&'static Regex> {
    static SHAPES_RE: OnceLock<Option<Regex>> = OnceLock::new();
    SHAPES_RE.get_or_init(|| Regex::new(&SHAPES.join("|")).ok()).as_ref()
}

fn valued() -> &'static [(Regex, bool)] {
    static VALUED_RE: OnceLock<Vec<(Regex, bool)>> = OnceLock::new();
    VALUED_RE.get_or_init(|| {
        valued_patterns().into_iter().filter_map(|(p, mixed)| Some((Regex::new(&p).ok()?, mixed))).collect()
    })
}

/// Os valores que têm letra e número sem serem segredo: o código de um item
/// (`MSTD-TASK-0101`), a data com hora e o caminho de arquivo, com ou sem a
/// linha (`apps/rt/src/shared/rtk_gain.rs:120`).
fn not_secret() -> Option<&'static Regex> {
    static NOT_SECRET_RE: OnceLock<Option<Regex>> = OnceLock::new();
    NOT_SECRET_RE
        .get_or_init(|| {
            Regex::new(concat!(
                r"^(?:",
                r"[A-Z][A-Z0-9]*-[A-Z]+-\d+",
                r"|\d{4}-\d{2}-\d{2}.*",
                r"|(?:[\w.@-]+/)+[\w@-]+\.[A-Za-z0-9]{1,6}(?::\d+){0,2}",
                r"|(?:/|\./|\.\./|~/)[\w.@/-]+(?::\d+){0,2}",
                r")$",
            ))
            .ok()
        })
        .as_ref()
}

/// O texto tem algo com cara de segredo.
pub(super) fn looks_like_secret(text: &str) -> bool {
    shapes().is_some_and(|re| re.is_match(text))
        || valued().iter().any(|(re, mixed)| {
            re.captures_iter(text)
                .any(|caps| caps.name("v").is_some_and(|value| real_value(value.as_str(), *mixed)))
        })
}

/// Um valor que parece de verdade: não é um marcador de lugar, uma palavra de
/// exemplo, um código de item, uma data, um caminho ou a leitura de uma
/// variável de ambiente, e, quando `mixed`, tem letra e número.
fn real_value(value: &str, mixed: bool) -> bool {
    let value = value.trim_end_matches(['.', ',', ';', ':', ')', ']', '}']);
    let lower = value.to_ascii_lowercase();
    let placeholder = value.starts_with(['$', '{', '%', '*', '.', '<', '['])
        || value.chars().all(|c| c == value.chars().next().unwrap_or('x'))
        || lower.contains("xxxx")
        || value.contains('…')
        || ENV_READS.iter().any(|read| lower.starts_with(read))
        || EXAMPLE_WORDS.iter().any(|word| lower == *word);
    let letter = value.chars().any(char::is_alphabetic);
    let digit = value.chars().any(|c| c.is_ascii_digit());
    let ordinary = not_secret().is_some_and(|re| re.is_match(value));
    (!mixed || letter && digit) && !placeholder && !ordinary
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cada família conhecida casa; o mesmo assunto escrito em prosa, um nome
    /// de campo, um marcador e um exemplo curto não casam.
    #[test]
    fn keys_tokens_and_passwords_are_found_and_prose_is_not() {
        let found = [
            "a chave AKIAIOSFODNN7EXAMPLE vazou",
            &format!("token ghp_{}", "a1".repeat(18)),
            &format!("glpat-{}", "x1y2".repeat(5)),
            &format!("sk_live_{}", "4eC39HqLyjWDarjtT1zdp7dc"),
            "xoxb-123456789012-abcdef",
            &format!("sk-ant-api03-{}", "Zx9".repeat(8)),
            "-----BEGIN RSA PRIVATE KEY-----",
            "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U",
            "a senha: hunter2-segredo",
            "a senha é hunter2-segredo",
            "PASSWORD=S3nh4F0rte!",
            "api_key = \"abc123def456\"",
        ];
        for text in found {
            assert!(looks_like_secret(text), "not found: {text}");
        }
        let prose = [
            "o binário procura texto com cara de segredo (chave, token, senha)",
            "a senha: <senha>",
            "token: ${GITHUB_TOKEN}",
            "password=********",
            "a senha é o campo do formulário",
            "the password is required",
            "o campo `search` e o campo token: vazio",
            "secret: abcdefgh",
            "MSTD-RULE-0008 e sk-curto",
        ];
        for text in prose {
            assert!(!looks_like_secret(text), "false positive: {text}");
        }
    }

    /// As formas comuns de escrever um segredo casam: o nome colado ao fim de
    /// outro, a aspa do JSON antes dos dois-pontos, o segredo do cliente, a
    /// senha num endereço de banco, o token depois de `Bearer`, as chaves
    /// novas com `_` e `-` e o token do npm.
    #[test]
    fn the_common_ways_of_writing_a_secret_are_found() {
        let found = [
            "DB_PASSWORD=S3nh4F0rte2024",
            "export GITHUB_TOKEN=a1b2c3d4e5f6g7h8i9j0",
            "AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
            r#"{"password": "S3nh4F0rte!"}"#,
            r#"{"db_password":"S3nh4F0rte!"}"#,
            "client_secret=9f8e7d6c5b4a3210",
            "client-secret: 9f8e7d6c5b4a3210",
            "CLIENT_SECRET=\"9f8e7d6c5b4a3210\"",
            "o banco fica em postgres://loja:S3nh4F0rte@db.interno:5432/loja",
            "mongodb+srv://admin:Xk29dmQ@cluster0.example.net/app",
            "Authorization: Bearer 8f14e45fceea167a5a36dedd4bea2543",
            &format!("sk-proj-{}", "Ab_3-".repeat(8)),
            &format!("sk-svcacct-{}", "Zz9_q-".repeat(6)),
            &format!("sk-admin-{}", "k7-Q_".repeat(8)),
            &format!("//registry.npmjs.org/:_authToken=npm_{}", "a1B2c3".repeat(6)),
            &format!("npm_{}", "a1B2c3".repeat(6)),
            "a senha do banco: S3nh4F0rte",
            "a senha do banco de produção é S3nh4F0rte",
        ];
        for text in found {
            assert!(looks_like_secret(text), "not found: {text}");
        }
    }

    /// O que tem letra e número sem ser segredo não casa: o código de um
    /// item, a data, o caminho com linha, a leitura de uma variável de
    /// ambiente e o endereço com usuário e senha de exemplo.
    #[test]
    fn codes_dates_paths_and_environment_reads_are_not_secrets() {
        let ordinary = [
            "token: MSTD-TASK-0101",
            "token: MSTD-TASK-0101, MSTD-TASK-0102",
            "segredo: MSTD-DEC-0138.",
            "token: 2026-09-17",
            "secret: 2026-09-17T02:35:53-03:00",
            "token: apps/rt/src/shared/rtk_gain.rs:120",
            "secret: apps/…/rtk_gain.rs:120",
            "pwd: /home/user/projeto2",
            "senha: ./config/secrets2.json",
            "token: process.env.GITHUB_TOKEN2",
            "GITHUB_TOKEN=process.env.TOKEN_V2",
            "password = os.environ[\"DB_PASSWORD_2\"]",
            "a forma é postgres://usuário:senha@host",
            "postgres://user:<senha>@host",
            "Bearer <token>",
            "Authorization: Bearer ${TOKEN}",
            "DB_PASSWORD=…, GITHUB_TOKEN=…",
            r#""password": "…""#,
            "sk-proj-… e npm_…",
            "o texto colocado pelos ganchos: 1234 tokens",
            "https://claude.ai/code/artifact/abc123:8080",
        ];
        for text in ordinary {
            assert!(!looks_like_secret(text), "false positive: {text}");
        }
    }

    /// A chave anunciada pelo nome casa, com o nome sozinho ou no fim de
    /// outra palavra, em inglês e em português: a variável de ambiente, o
    /// campo do appsettings, a cadeia de conexão do Azure e a frase. O mesmo
    /// nome diante de um código, uma data, um caminho, um marcador, uma
    /// leitura de configuração ou de prosa não casa, nem a palavra que só
    /// começa pelo nome.
    #[test]
    fn a_key_named_key_or_chave_is_found_and_the_rest_is_not() {
        let found = [
            "SECRET_KEY=a1b2c3d4e5f6g7h8",
            r#"{"SecretKey": "Xk29dmQpL7wz"}"#,
            r#""Jwt": {"Issuer": "loja", "Key": "Xk29dmQpL7wz"}"#,
            "DefaultEndpointsProtocol=https;AccountName=loja;AccountKey=Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw==;EndpointSuffix=core.windows.net",
            "ENCRYPTION_KEY=9f8e7d6c5b4a3210",
            "a chave é Xk29dmQpL7wz",
            "chave: Xk29dmQpL7wz",
            "key = \"Xk29dmQpL7wz\"",
        ];
        for text in found {
            assert!(looks_like_secret(text), "not found: {text}");
        }
        let ordinary = [
            r#""keys": ["banco", "senha"]"#,
            "chave: MSTD-DEC-0138",
            "key: 2026-09-17",
            "chave: apps/rt/src/commands/spec_events/pages/secret.rs:41",
            "SECRET_KEY=${SECRET_KEY}",
            "SECRET_KEY=…",
            "ENCRYPTION_KEY=process.env.ENCRYPTION_KEY2",
            r#"options.SecretKey = configuration["Jwt:SecretKey"];"#,
            r#"builder.Configuration["Jwt:Key"]"#,
            "a chave é o código do item",
            "a chave primária é id_cliente",
            "the key is required",
            "keyboard: abnt2-br",
        ];
        for text in ordinary {
            assert!(!looks_like_secret(text), "false positive: {text}");
        }
    }

    /// A senha dentro de um endereço casa mesmo sem número e com o usuário
    /// vazio. Continuam de fora o marcador e a palavra de exemplo no lugar da
    /// senha, e o endereço sem senha.
    #[test]
    fn a_password_inside_an_address_is_found_without_a_digit_or_a_user() {
        let found = [
            "postgres://app:senhaforte@db",
            "redis://:S3nh4F0rte@cache",
            "mysql://root:minhasenha@localhost:3306/loja",
            "amqp://:guestguest@rabbit",
        ];
        for text in found {
            assert!(looks_like_secret(text), "not found: {text}");
        }
        let ordinary = [
            "a forma é postgres://usuário:senha@host",
            "postgres://usuario:senha@host",
            "por isso postgres://u:senha@host não é pego",
            "postgres://user:password@localhost",
            "mysql://root:pass@db",
            "mongodb://admin:pwd@host",
            "mongodb://admin:secret@host",
            "http://app:token@host",
            "http://app:user@host",
            "http://app:usuário@host",
            "redis://:<senha>@cache",
            "redis://:${REDIS_PASSWORD}@cache",
            "amqp://user:****@rabbit",
            "amqp://user:xxxx@rabbit",
            "postgres://app:…@db",
            "ssh://git@github.com:22/org/repo",
            "https://claude.ai/code/artifact/abc123:8080",
        ];
        for text in ordinary {
            assert!(!looks_like_secret(text), "false positive: {text}");
        }
    }
}
