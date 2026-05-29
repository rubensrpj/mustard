//! `language_caps` — deterministic, data-driven language *capabilities* keyed
//! by stack-id.
//!
//! Where [`super::frameworks`] scans source *content* for stack signals, this
//! module answers a structural question the cluster-discovery pass needs before
//! it scans anything: *what syntactic features does this stack-id have?* —
//! specifically
//!
//! - **decorator syntax** (`@Name`-style annotations prefixing a declaration),
//! - the **declaration keywords** a decorator may prefix
//!   (`class` / `function` / `def` / `interface` / `fun`),
//! - **function-prefix** detection applicability (camelCase / snake_case
//!   leading-prefix clustering), and
//! - **base-class syntax** (the `class ` / `extends ` keyword pair).
//!
//! These were previously hard-coded as `matches!(stack_id, "typescript" |
//! "python" | ...)` gates and literal `"class "` / `"extends "` strings inside
//! `apps/rt/src/commands/scan/cluster_discovery.rs`. The invariant is the same
//! one [`super::frameworks`] enforces: the *values* live in DATA (a TOML
//! catalogue) while the *logic* (how a gate is consulted, how clustering runs)
//! stays generic and language-agnostic.
//!
//! ## Built-in base + on-disk override
//!
//! The base catalogue is embedded via [`include_str!`] from
//! `language_caps_builtin.toml`, so capability lookup works offline. A project
//! may override it by dropping `.claude/vocab/{name}.toml` (default name
//! `language_caps`); when that file exists it **replaces** the built-in base
//! wholesale — see [`LanguageCapabilities::load`]. This mirrors the
//! built-in-plus-override policy [`super::frameworks::FrameworkVocabulary`]
//! already ships.
//!
//! ## Agnostic floor
//!
//! A stack-id with no `[[lang]]` entry reports *no* capabilities:
//! [`LanguageCapabilities::has_decorators`] / [`has_fn_prefix`] return `false`,
//! [`decl_keywords`] returns an empty slice, and [`base_class_syntax`] returns
//! `None`. Mustard never invents a capability it was not told about.
//!
//! [`has_fn_prefix`]: LanguageCapabilities::has_fn_prefix
//! [`decl_keywords`]: LanguageCapabilities::decl_keywords
//! [`base_class_syntax`]: LanguageCapabilities::base_class_syntax

use super::VocabError;
use serde::Deserialize;
use std::path::Path;

/// The built-in language-capabilities catalogue, embedded at compile time. The
/// *guaranteed base*; an on-disk vocab overrides it (see
/// [`LanguageCapabilities::load`]).
const BUILTIN_LANGUAGE_CAPS_TOML: &str = include_str!("language_caps_builtin.toml");

/// The default on-disk vocabulary name resolved under `.claude/vocab/`.
/// [`LanguageCapabilities::load`] looks for `.claude/vocab/language_caps.toml`.
pub const DEFAULT_LANGUAGE_CAPS_NAME: &str = "language_caps";

// ---------------------------------------------------------------------------
// Schema
// ---------------------------------------------------------------------------

/// One stack-id's capabilities — the deserialised shape of a single `[[lang]]`
/// table-array entry. Every capability field defaults to "absent" so a terse
/// entry (just `id`) is legal and reports the agnostic floor.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct LanguageCaps {
    /// The stack-id key this entry describes (e.g. `"typescript"`,
    /// `"python"`). Matched case-sensitively by the accessors.
    pub id: String,
    /// Whether the language has `@Name`-style decorator syntax that can
    /// prefix a declaration. Defaults to `false` (agnostic floor).
    #[serde(default)]
    pub has_decorators: bool,
    /// The declaration keywords a decorator may prefix (e.g. `class`,
    /// `function`, `def`, `interface`, `fun`). Empty when unset.
    #[serde(default)]
    pub decl_keywords: Vec<String>,
    /// Whether function-prefix clustering applies to this stack. Defaults to
    /// `false`.
    #[serde(default)]
    pub has_fn_prefix: bool,
    /// The class-declaration keyword used by base-class clustering (e.g.
    /// `"class "`). `None` ⇒ no base-class syntax for this stack.
    #[serde(default)]
    pub base_class_class_kw: Option<String>,
    /// The inheritance keyword following the class name (e.g. `"extends "`).
    /// Both this and [`Self::base_class_class_kw`] must be present for
    /// [`LanguageCapabilities::base_class_syntax`] to return a pair.
    #[serde(default)]
    pub base_class_extends_kw: Option<String>,
    /// Extra source extensions the filename-cluster pass scans in addition to
    /// the primary one (e.g. `".tsx"` for typescript). Empty ⇒ primary only.
    #[serde(default)]
    pub secondary_exts: Vec<String>,
}

/// Top-level document deserialised from a language-capabilities TOML. The
/// `[[lang]]` table array is the only key.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct LanguageCapsDoc {
    /// Every `[[lang]]` table entry, in document order. A later entry with a
    /// duplicate `id` shadows an earlier one (last wins), matching how a
    /// hand-edited override would expect to "fix" a single stack.
    #[serde(default, rename = "lang")]
    pub langs: Vec<LanguageCaps>,
}

impl LanguageCapsDoc {
    /// Parse a language-capabilities TOML document. Pure on `&str`.
    ///
    /// # Errors
    /// Returns [`VocabError::InvalidToml`] when the input cannot be
    /// deserialised (malformed table array, wrong field type, …).
    pub fn parse_str(raw: &str) -> Result<Self, VocabError> {
        toml::from_str::<Self>(raw).map_err(|e| VocabError::InvalidToml(e.to_string()))
    }
}

// ---------------------------------------------------------------------------
// Capability lookup
// ---------------------------------------------------------------------------

/// A built language-capability table: stack-id → [`LanguageCaps`]. Construct
/// via [`LanguageCapabilities::builtin`], [`LanguageCapabilities::from_doc`],
/// or [`LanguageCapabilities::load`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LanguageCapabilities {
    entries: Vec<LanguageCaps>,
}

impl LanguageCapabilities {
    /// Build a capability table from a parsed document. Entries are kept in
    /// document order; the accessors resolve a stack with [`Self::lookup`],
    /// which honours last-wins for duplicate ids.
    #[must_use]
    pub fn from_doc(doc: LanguageCapsDoc) -> Self {
        Self {
            entries: doc.langs,
        }
    }

    /// Build the table from the embedded built-in catalogue. Infallible in
    /// practice (the embedded TOML is validated by a unit test), but the
    /// constructor still surfaces a typed error rather than panicking so the
    /// `unwrap_used = deny` contract holds at every call site.
    ///
    /// # Errors
    /// Returns [`VocabError::InvalidToml`] if the embedded TOML ever fails to
    /// parse.
    pub fn builtin() -> Result<Self, VocabError> {
        let doc = LanguageCapsDoc::parse_str(BUILTIN_LANGUAGE_CAPS_TOML)?;
        Ok(Self::from_doc(doc))
    }

    /// Load a *named* language-capabilities vocabulary, preferring an on-disk
    /// override over the built-in base.
    ///
    /// Resolution (identical policy to
    /// [`super::frameworks::FrameworkVocabulary::load`]):
    /// 1. If `{project_root}/.claude/vocab/{name}.toml` exists and parses, it
    ///    **replaces** the built-in base wholesale and is used as-is.
    /// 2. Otherwise (file absent) the embedded built-in base is used — but
    ///    only when `name` is [`DEFAULT_LANGUAGE_CAPS_NAME`]; a named vocab
    ///    that does not exist on disk is a [`VocabError::FileNotFound`],
    ///    because silently substituting the capability base for an unrelated
    ///    named vocab would hide a misconfiguration.
    ///
    /// A file that exists but fails to parse surfaces the parse error (it is
    /// *not* fail-open to the built-in): a malformed override is a real
    /// configuration bug the caller should see.
    ///
    /// # Errors
    /// [`VocabError::FileNotFound`] (named vocab absent, non-default name),
    /// [`VocabError::InvalidToml`] (override present but unparseable), or
    /// [`VocabError::Io`] (read failure).
    pub fn load(name: &str, project_root: &Path) -> Result<Self, VocabError> {
        let path = project_root
            .join(".claude")
            .join("vocab")
            .join(format!("{name}.toml"));

        match std::fs::read_to_string(&path) {
            Ok(raw) => {
                let doc = LanguageCapsDoc::parse_str(&raw)?;
                Ok(Self::from_doc(doc))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if name == DEFAULT_LANGUAGE_CAPS_NAME {
                    Self::builtin()
                } else {
                    Err(VocabError::FileNotFound(path.display().to_string()))
                }
            }
            Err(e) => Err(VocabError::Io(e.to_string())),
        }
    }

    /// Resolve a stack-id to its capability entry. Last-wins on duplicate ids
    /// (a hand-edited override "fixing" one stack appends a corrected entry).
    /// Returns `None` for an unknown stack — the accessors translate that into
    /// the agnostic floor.
    #[must_use]
    fn lookup(&self, stack: &str) -> Option<&LanguageCaps> {
        self.entries.iter().rev().find(|e| e.id == stack)
    }

    /// Whether `stack` has `@Name`-style decorator syntax. `false` for an
    /// unknown stack (agnostic floor).
    #[must_use]
    pub fn has_decorators(&self, stack: &str) -> bool {
        self.lookup(stack).is_some_and(|c| c.has_decorators)
    }

    /// The declaration keywords a decorator may prefix for `stack`, borrowed in
    /// document order. Empty slice for an unknown stack (agnostic floor).
    #[must_use]
    pub fn decl_keywords(&self, stack: &str) -> &[String] {
        self.lookup(stack).map_or(&[], |c| c.decl_keywords.as_slice())
    }

    /// Whether function-prefix clustering applies to `stack`. `false` for an
    /// unknown stack (agnostic floor).
    #[must_use]
    pub fn has_fn_prefix(&self, stack: &str) -> bool {
        self.lookup(stack).is_some_and(|c| c.has_fn_prefix)
    }

    /// The `(class_kw, extends_kw)` pair for `stack`'s base-class syntax, or
    /// `None` when the stack has no base-class syntax (agnostic floor) — i.e.
    /// either field is absent.
    #[must_use]
    pub fn base_class_syntax(&self, stack: &str) -> Option<(&str, &str)> {
        let c = self.lookup(stack)?;
        match (&c.base_class_class_kw, &c.base_class_extends_kw) {
            (Some(class_kw), Some(extends_kw)) => Some((class_kw.as_str(), extends_kw.as_str())),
            _ => None,
        }
    }

    /// Extra source extensions the filename-cluster pass should scan for
    /// `stack` in addition to the primary one. Empty slice for an unknown stack
    /// (agnostic floor).
    #[must_use]
    pub fn secondary_exts(&self, stack: &str) -> &[String] {
        self.lookup(stack).map_or(&[], |c| c.secondary_exts.as_slice())
    }

    /// Number of stack-id entries in the table. Diagnostics / tests.
    #[must_use]
    pub fn stack_count(&self) -> usize {
        self.entries.len()
    }
}

/// Resolve the language-capability table, honouring a project-local override.
/// Resolves via [`LanguageCapabilities::load`]`(`[`DEFAULT_LANGUAGE_CAPS_NAME`]`,
/// root)` so a `.claude/vocab/language_caps.toml` under `root` **replaces** the
/// built-in base, while a project with no override falls back to it.
///
/// Fail-open: a missing or malformed override degrades to the **built-in**
/// base (never panics, never an empty table) so cluster discovery keeps the
/// capabilities the binary shipped with rather than silently losing them. A
/// caller that needs to distinguish "override present but broken" from "no
/// override" should call [`LanguageCapabilities::load`] directly.
#[must_use]
pub fn language_capabilities_with(root: &Path) -> LanguageCapabilities {
    LanguageCapabilities::load(DEFAULT_LANGUAGE_CAPS_NAME, root)
        .or_else(|_| LanguageCapabilities::builtin())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Built-in catalogue
    // -----------------------------------------------------------------------

    #[test]
    fn builtin_toml_parses_and_builds() {
        let caps = LanguageCapabilities::builtin().expect("built-in language caps build");
        assert!(caps.stack_count() > 0);
    }

    #[test]
    fn builtin_has_the_known_decorator_stacks() {
        let caps = LanguageCapabilities::builtin().unwrap();
        // EXACTLY the stacks the cluster-discovery decorator gate hard-coded.
        for stack in ["typescript", "python", "java", "kotlin"] {
            assert!(
                caps.has_decorators(stack),
                "{stack} should have decorator syntax"
            );
        }
    }

    #[test]
    fn builtin_decl_keywords_match_the_ported_array() {
        let caps = LanguageCapabilities::builtin().unwrap();
        // The literal declaration-keyword array moved out of cluster_discovery.
        let expected = ["class", "function", "def", "interface", "fun"];
        for stack in ["typescript", "python", "java", "kotlin"] {
            let kws: Vec<&str> = caps.decl_keywords(stack).iter().map(String::as_str).collect();
            assert_eq!(kws, expected, "{stack} decl keywords");
        }
    }

    #[test]
    fn builtin_has_the_known_fn_prefix_stacks() {
        let caps = LanguageCapabilities::builtin().unwrap();
        // EXACTLY the stacks the fn-prefix gate hard-coded.
        assert!(caps.has_fn_prefix("typescript"));
        assert!(caps.has_fn_prefix("python"));
        // Java/Kotlin have decorators but NOT fn-prefix detection.
        assert!(!caps.has_fn_prefix("java"));
        assert!(!caps.has_fn_prefix("kotlin"));
    }

    #[test]
    fn builtin_base_class_syntax_is_typescript_only() {
        let caps = LanguageCapabilities::builtin().unwrap();
        // TS-only, with the exact keyword pair the TS scanner used.
        assert_eq!(
            caps.base_class_syntax("typescript"),
            Some(("class ", "extends "))
        );
        // No other built-in stack declares base-class syntax.
        assert_eq!(caps.base_class_syntax("python"), None);
        assert_eq!(caps.base_class_syntax("java"), None);
        assert_eq!(caps.base_class_syntax("kotlin"), None);
    }

    // -----------------------------------------------------------------------
    // Agnostic floor — an unknown stack reports nothing
    // -----------------------------------------------------------------------

    #[test]
    fn builtin_secondary_exts_is_typescript_tsx_only() {
        let caps = LanguageCapabilities::builtin().unwrap();
        // TS scans `.tsx` in addition to its primary `.ts` (the gate the
        // filename-cluster pass used to hard-code as `stack_id == "typescript"`).
        assert_eq!(caps.secondary_exts("typescript"), &[".tsx".to_string()]);
        // No other built-in stack declares a secondary extension.
        assert!(caps.secondary_exts("python").is_empty());
        assert!(caps.secondary_exts("java").is_empty());
    }

    #[test]
    fn unknown_stack_reports_the_agnostic_floor() {
        let caps = LanguageCapabilities::builtin().unwrap();
        assert!(!caps.has_decorators("rust"));
        assert!(!caps.has_fn_prefix("go"));
        assert!(caps.decl_keywords("elixir").is_empty());
        assert_eq!(caps.base_class_syntax("haskell"), None);
        assert!(caps.secondary_exts("rust").is_empty());
    }

    // -----------------------------------------------------------------------
    // Doc parsing
    // -----------------------------------------------------------------------

    #[test]
    fn doc_parses_minimal_entry_to_floor() {
        // An entry with only an id reports the floor — every capability absent.
        let doc = LanguageCapsDoc::parse_str(
            r#"
[[lang]]
id = "minimal"
"#,
        )
        .unwrap();
        let caps = LanguageCapabilities::from_doc(doc);
        assert!(!caps.has_decorators("minimal"));
        assert!(!caps.has_fn_prefix("minimal"));
        assert!(caps.decl_keywords("minimal").is_empty());
        assert_eq!(caps.base_class_syntax("minimal"), None);
    }

    #[test]
    fn doc_parse_rejects_wrong_field_type() {
        let toml = r#"
[[lang]]
id = "typescript"
has_decorators = "yes-please"
"#;
        let err = LanguageCapsDoc::parse_str(toml).unwrap_err();
        assert!(matches!(err, VocabError::InvalidToml(_)));
    }

    #[test]
    fn partial_base_class_yields_none() {
        // Only one of the two keys present ⇒ no base-class syntax.
        let doc = LanguageCapsDoc::parse_str(
            r#"
[[lang]]
id = "half"
base_class_class_kw = "class "
"#,
        )
        .unwrap();
        let caps = LanguageCapabilities::from_doc(doc);
        assert_eq!(caps.base_class_syntax("half"), None);
    }

    // -----------------------------------------------------------------------
    // Named load + on-disk override
    // -----------------------------------------------------------------------

    #[test]
    fn load_default_falls_back_to_builtin_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let caps = LanguageCapabilities::load(DEFAULT_LANGUAGE_CAPS_NAME, tmp.path())
            .expect("default name falls back to built-in");
        // Built-in base knows typescript has decorators.
        assert!(caps.has_decorators("typescript"));
    }

    #[test]
    fn load_non_default_named_vocab_errors_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let err = LanguageCapabilities::load("custom-caps", tmp.path()).unwrap_err();
        assert!(matches!(err, VocabError::FileNotFound(_)));
    }

    #[test]
    fn on_disk_override_replaces_builtin() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".claude").join("vocab");
        std::fs::create_dir_all(&dir).unwrap();
        // An override that only knows about a bespoke stack.
        std::fs::write(
            dir.join("language_caps.toml"),
            r#"
[[lang]]
id = "mylang"
has_decorators = true
decl_keywords = ["klass"]
has_fn_prefix = true
base_class_class_kw = "klass "
base_class_extends_kw = "inherits "
"#,
        )
        .unwrap();

        let caps = LanguageCapabilities::load(DEFAULT_LANGUAGE_CAPS_NAME, tmp.path()).unwrap();
        // The override IS respected: its bespoke stack resolves...
        assert!(caps.has_decorators("mylang"));
        assert_eq!(caps.decl_keywords("mylang"), &["klass".to_string()]);
        assert!(caps.has_fn_prefix("mylang"));
        assert_eq!(
            caps.base_class_syntax("mylang"),
            Some(("klass ", "inherits "))
        );
        // ...and the built-in base is fully replaced: typescript no longer known.
        assert!(!caps.has_decorators("typescript"));
        assert_eq!(caps.base_class_syntax("typescript"), None);
    }

    #[test]
    fn on_disk_override_parse_error_is_surfaced() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".claude").join("vocab");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("language_caps.toml"), "this is = = not toml").unwrap();
        let err =
            LanguageCapabilities::load(DEFAULT_LANGUAGE_CAPS_NAME, tmp.path()).unwrap_err();
        assert!(matches!(err, VocabError::InvalidToml(_)));
    }

    // -----------------------------------------------------------------------
    // Fail-open helper
    // -----------------------------------------------------------------------

    #[test]
    fn with_falls_back_to_builtin_when_no_override() {
        let tmp = tempfile::tempdir().unwrap();
        let caps = language_capabilities_with(tmp.path());
        assert!(caps.has_decorators("typescript"));
    }

    #[test]
    fn with_honours_on_disk_override() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".claude").join("vocab");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("language_caps.toml"),
            r#"
[[lang]]
id = "mylang"
has_decorators = true
"#,
        )
        .unwrap();
        let caps = language_capabilities_with(tmp.path());
        assert!(caps.has_decorators("mylang"));
        // Override replaces the base wholesale.
        assert!(!caps.has_decorators("typescript"));
    }

    #[test]
    fn with_is_fail_open_to_builtin_on_malformed_override() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".claude").join("vocab");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("language_caps.toml"), "this is = = not toml").unwrap();
        // Unlike `load`, the fail-open helper degrades to the built-in base.
        let caps = language_capabilities_with(tmp.path());
        assert!(caps.has_decorators("typescript"));
    }
}
