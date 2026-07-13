//! `wasm_acquire` — on-demand WASM grammar acquisition + versioned cache.
//!
//! **Entire module is gated behind `#[cfg(feature = "wasm-grammars")]`.** With
//! the feature off the file compiles to nothing; the native (in-crate +
//! `tree_sitter_loader`) path in [`super::loader`] and the textual floor in
//! [`super::entity`] cover everything, byte-for-byte identical to a build
//! without the feature.
//!
//! ## What this is
//!
//! The third tier of the agnostic grammar strategy (see the crate-level note in
//! [`super`]):
//!
//! 1. **In-crate** common grammars (Rust/TS/TSX/Python/Go/Java/C#) — offline,
//!    compiled into the binary by [`super::loader::builtin_grammars`].
//! 2. **WASM-on-demand** (this module) — for languages *outside* the in-crate
//!    set (Ruby, C, C++, PHP, Bash, …). A pinned registry maps a `lang_id` to a
//!    versioned `tree-sitter-<lang>.wasm` URL; the blob is fetched once, sha256
//!    is verified (when pinned) and the result is cached under
//!    `~/.mustard/grammars/{lang}/{version}/`.
//! 3. **Textual floor** — Aho-Corasick keyword scan when neither tier resolves.
//!
//! ## Fail-open contract
//!
//! Every public entry point returns `Option` and degrades to `None` on *any*
//! error (no home dir, network failure, sha mismatch, malformed manifest, ABI
//! incompatibility, …). A `None` simply means "no WASM grammar"; the caller
//! falls through to the textual floor. Nothing here panics.
//!
//! ## Cache layout
//!
//! ```text
//! ~/.mustard/grammars/
//!   ruby/
//!     0.23.1/
//!       grammar.wasm
//!       manifest.json   { version, sha256, source_url, abi }
//! ```
//!
//! The `manifest.json` records the *observed* sha256 even when the registry
//! pins none, so a later run can detect a corrupted/partial download by
//! re-hashing the cached blob against its own manifest.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use tree_sitter::wasmtime::Engine;
use tree_sitter::{Language, WasmStore};

/// One pinned grammar in the acquisition registry.
///
/// `sha256` is optional: when present it is enforced (a mismatch fails open to
/// `None`); when absent the observed digest is recorded in the cache manifest
/// so a corrupted cache entry is still detectable on re-read.
struct WasmGrammarPin {
    /// Canonical lang id. Matches the `lang_id` callers pass to
    /// [`super::extract_entities`] / [`super::TreeSitterParser::for_language`].
    lang_id: &'static str,
    /// Pinned grammar version (the `tree-sitter-wasms` package version the URL
    /// points at). Used verbatim as the cache-directory segment.
    version: &'static str,
    /// The `tree-sitter-wasms` blob stem, e.g. `tree-sitter-ruby` (without the
    /// `.wasm` suffix). The full pinned URL is assembled by [`pin_url`].
    blob_stem: &'static str,
    /// Optional pinned sha256 (lowercase hex). Validated when present.
    sha256: Option<&'static str>,
}

/// The `tree-sitter-wasms` package version every URL is pinned to. Bumping this
/// constant (plus clearing the cache) is how the grammar set is upgraded.
const WASMS_PKG_VERSION: &str = "0.1.12";

/// Pinned grammar registry — languages **outside** the in-crate built-in set
/// (Rust/TS/TSX/Python/Go/Java/C#).
///
/// Source: `tree-sitter-wasms` on unpkg — a community package that publishes
/// pre-built `tree-sitter-<lang>.wasm` blobs for the standard grammar set. URLs
/// are pinned to an exact package version ([`WASMS_PKG_VERSION`]) so a cache
/// directory is reproducible. The `version` segment mirrors the package version
/// (every blob in a release shares it); per-grammar upstream versions are not
/// separately tracked here because the cache only needs a stable,
/// monotonically-bumpable key.
///
/// `sha256` is left `None` for now (the observed digest is persisted on first
/// fetch); a follow-up can pin the digests once they are recorded from a
/// trusted run. Leaving them `None` keeps the registry honest rather than
/// shipping unverified-but-pinned hashes.
const REGISTRY: &[WasmGrammarPin] = &[
    pin("ruby", "tree-sitter-ruby"),
    pin("c", "tree-sitter-c"),
    pin("cpp", "tree-sitter-cpp"),
    pin("php", "tree-sitter-php"),
    pin("bash", "tree-sitter-bash"),
    pin("scala", "tree-sitter-scala"),
    pin("kotlin", "tree-sitter-kotlin"),
    pin("swift", "tree-sitter-swift"),
    pin("elixir", "tree-sitter-elixir"),
    pin("lua", "tree-sitter-lua"),
    pin("haskell", "tree-sitter-haskell"),
    pin("javascript", "tree-sitter-javascript"),
];

/// Build a [`WasmGrammarPin`] from a lang id and the `tree-sitter-wasms` blob
/// stem. `const fn` so [`REGISTRY`] stays a compile-time constant; the sha is
/// `None` (recorded on first fetch) and the version is [`WASMS_PKG_VERSION`].
const fn pin(lang_id: &'static str, blob_stem: &'static str) -> WasmGrammarPin {
    WasmGrammarPin {
        lang_id,
        version: WASMS_PKG_VERSION,
        blob_stem,
        sha256: None,
    }
}

/// Look up the pinned entry for `lang_id`, if the registry covers it.
fn registry_entry(lang_id: &str) -> Option<&'static WasmGrammarPin> {
    REGISTRY.iter().find(|p| p.lang_id == lang_id)
}

/// Assemble the full pinned unpkg URL for a registry entry:
/// `https://unpkg.com/tree-sitter-wasms@<version>/out/<blob_stem>.wasm`.
fn pin_url(pin: &WasmGrammarPin) -> String {
    format!(
        "https://unpkg.com/tree-sitter-wasms@{}/out/{}.wasm",
        pin.version, pin.blob_stem
    )
}

/// On-disk cache manifest sitting beside `grammar.wasm`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheManifest {
    /// Pinned grammar version (the cache-dir segment).
    version: String,
    /// Lowercase-hex sha256 of the cached `grammar.wasm`. Always populated on
    /// write (observed digest), even when the registry pins none.
    sha256: String,
    /// The URL the blob was fetched from.
    source_url: String,
    /// ABI marker. Recorded for diagnostics / future ABI-gating; currently the
    /// string `"wasm"`.
    abi: String,
}

/// Acquire the raw WASM bytes for `lang_id`, using the versioned cache when
/// possible and the pinned network URL otherwise. **Fail-open**: returns `None`
/// on any error (unknown lang, no home dir, network/sha/io failure).
///
/// Cache hit path: if `grammar.wasm` + `manifest.json` exist and the on-disk
/// bytes hash to the manifest's `sha256` (and, when the registry pins a sha,
/// that pin matches too), the cached bytes are returned without touching the
/// network.
///
/// Cache miss path: download from the pinned URL, validate the pinned sha (when
/// present), persist `grammar.wasm` + `manifest.json`, and return the bytes.
pub(crate) fn acquire_wasm_bytes(lang_id: &str) -> Option<Vec<u8>> {
    let pin = registry_entry(lang_id)?;
    let dir = cache_dir(lang_id, pin.version)?;
    let wasm_path = dir.join("grammar.wasm");
    let manifest_path = dir.join("manifest.json");

    // --- Cache hit ---------------------------------------------------------
    if let Some(bytes) = read_valid_cache(&wasm_path, &manifest_path, pin) {
        return Some(bytes);
    }

    // --- Cache miss: download ---------------------------------------------
    let url = pin_url(pin);
    let bytes = download(&url)?;

    let observed = sha256_hex(&bytes);
    if let Some(expected) = pin.sha256 {
        if !observed.eq_ignore_ascii_case(expected) {
            // Pinned sha mismatch — refuse the blob, fail open.
            return None;
        }
    }

    // Persist cache + manifest. A write failure is non-fatal: we still return
    // the freshly-downloaded bytes (fail-open also applies to the cache write).
    let _ = std::fs::create_dir_all(&dir);
    if std::fs::write(&wasm_path, &bytes).is_ok() {
        let manifest = CacheManifest {
            version: pin.version.to_string(),
            sha256: observed,
            source_url: url,
            abi: "wasm".to_string(),
        };
        if let Ok(json) = serde_json::to_vec_pretty(&manifest) {
            let _ = std::fs::write(&manifest_path, json);
        }
    }

    Some(bytes)
}

/// Read the cached blob iff it exists and validates against its manifest (and
/// the registry-pinned sha, when present). Returns `None` on any mismatch so
/// the caller re-downloads.
fn read_valid_cache(
    wasm_path: &std::path::Path,
    manifest_path: &std::path::Path,
    pin: &WasmGrammarPin,
) -> Option<Vec<u8>> {
    let bytes = std::fs::read(wasm_path).ok()?;
    let manifest_raw = std::fs::read(manifest_path).ok()?;
    let manifest: CacheManifest = serde_json::from_slice(&manifest_raw).ok()?;

    let observed = sha256_hex(&bytes);
    // The cached bytes must match what the manifest recorded (detects a
    // truncated / corrupted blob).
    if !observed.eq_ignore_ascii_case(&manifest.sha256) {
        return None;
    }
    // When the registry pins a sha, it must also match.
    if let Some(expected) = pin.sha256 {
        if !observed.eq_ignore_ascii_case(expected) {
            return None;
        }
    }
    Some(bytes)
}

/// Acquire and load the WASM grammar for `lang_id` into a [`Language`] via a
/// `WasmStore` built from `engine`. **Fail-open**: `None` on any failure,
/// including an ABI-incompatible blob (`!language.is_wasm()` is impossible here
/// — a store-loaded language is always wasm — but we still assert it as a
/// guard).
///
/// The returned [`Language`] may be set on *any* [`tree_sitter::Parser`] whose
/// `WasmStore` was created from the **same** `engine` (the tree-sitter contract
/// — a language can cross stores that share an engine). [`super::loader`] caches
/// the returned `Language` keyed by `lang_id`.
pub fn acquire_language(engine: &Engine, lang_id: &str) -> Option<Language> {
    let bytes = acquire_wasm_bytes(lang_id)?;
    let mut store = WasmStore::new(engine).ok()?;
    let language = store.load_language(lang_id, &bytes).ok()?;
    // Guard: a store-loaded language must report as wasm. If somehow it does
    // not, the ABI is wrong for our wasm-store parser path — discard it.
    if !language.is_wasm() {
        return None;
    }
    Some(language)
}

/// `~/.mustard/grammars/{lang}/{version}`. `None` when no home dir resolves
/// (fail-open: the acquisition simply cannot cache and returns no grammar).
fn cache_dir(lang_id: &str, version: &str) -> Option<PathBuf> {
    Some(
        home_dir()?
            .join(".mustard")
            .join("grammars")
            .join(lang_id)
            .join(version),
    )
}

/// Resolve the user home dir cross-platform without a `dirs` dependency:
/// `USERPROFILE` on Windows, `HOME` elsewhere. Mirrors the rt-side helper.
fn home_dir() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Blocking HTTP GET returning the response body bytes. `None` on any transport
/// error, non-2xx status, or oversized body. A 16 MiB ceiling guards against a
/// hostile/misconfigured endpoint streaming an unbounded body (grammar blobs
/// are well under 4 MiB).
fn download(url: &str) -> Option<Vec<u8>> {
    const MAX_BYTES: u64 = 16 * 1024 * 1024;
    let mut response = ureq::get(url).call().ok()?;
    response
        .body_mut()
        .with_config()
        .limit(MAX_BYTES)
        .read_to_vec()
        .ok()
}

/// Lowercase-hex sha256 of `bytes`.
fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let digest = Sha256::digest(bytes);
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        let _ = write!(s, "{b:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The registry must never overlap the in-crate built-in set: WASM is the
    /// *complement* of the compiled-in grammars, not a duplicate path.
    #[test]
    fn registry_excludes_in_crate_builtins() {
        // The in-crate set (see `loader::builtin_grammars`).
        let builtins = ["rust", "typescript", "tsx", "python", "go", "java", "c-sharp"];
        for pin in REGISTRY {
            assert!(
                !builtins.contains(&pin.lang_id),
                "registry entry `{}` overlaps the in-crate built-in set",
                pin.lang_id
            );
        }
    }

    /// Every registry URL is HTTPS and pins the package version + a `.wasm`
    /// blob — no floating `latest`.
    #[test]
    fn registry_urls_are_pinned_https_wasm() {
        for pin in REGISTRY {
            let url = pin_url(pin);
            assert!(url.starts_with("https://"), "non-https url: {url}");
            assert!(url.ends_with(".wasm"), "non-wasm url: {url}");
            assert!(
                url.contains(&format!("@{}", pin.version)),
                "url not version-pinned: {url}"
            );
        }
    }

    /// `registry_entry` resolves a covered lang and rejects an in-crate one.
    #[test]
    fn registry_lookup_covers_complement_only() {
        assert!(registry_entry("ruby").is_some());
        assert!(registry_entry("cpp").is_some());
        assert!(registry_entry("rust").is_none(), "rust is in-crate, not wasm");
        assert!(registry_entry("totally-made-up").is_none());
    }

    /// sha256 of empty input is the well-known constant.
    #[test]
    fn sha256_hex_of_empty_is_known_constant() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    /// Cache round-trip with a SYNTHETIC blob (no network): write a fake
    /// `grammar.wasm` + a matching `manifest.json`, then prove `read_valid_cache`
    /// returns the bytes; corrupt the blob and prove it returns `None`.
    #[test]
    fn cache_round_trip_validates_sha() {
        let tmp = tempfile::tempdir().unwrap();
        let wasm_path = tmp.path().join("grammar.wasm");
        let manifest_path = tmp.path().join("manifest.json");

        let fake = b"\0asm\x01\x00\x00\x00 fake grammar bytes";
        std::fs::write(&wasm_path, fake).unwrap();
        let sha = sha256_hex(fake);
        let manifest = CacheManifest {
            version: "9.9.9".to_string(),
            sha256: sha.clone(),
            source_url: "https://example.invalid/x.wasm".to_string(),
            abi: "wasm".to_string(),
        };
        std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();

        // A pin with no expected sha — manifest match alone suffices.
        let p = WasmGrammarPin {
            lang_id: "fake",
            version: "9.9.9",
            blob_stem: "fake",
            sha256: None,
        };
        let got = read_valid_cache(&wasm_path, &manifest_path, &p).expect("cache hit");
        assert_eq!(got, fake);

        // A pin WITH a matching expected sha — still a hit.
        let leaked: &'static str = Box::leak(sha.clone().into_boxed_str());
        let p_ok = WasmGrammarPin {
            lang_id: "fake",
            version: "9.9.9",
            blob_stem: "fake",
            sha256: Some(leaked),
        };
        assert!(read_valid_cache(&wasm_path, &manifest_path, &p_ok).is_some());

        // A pin with a WRONG expected sha — miss.
        let p_bad = WasmGrammarPin {
            lang_id: "fake",
            version: "9.9.9",
            blob_stem: "fake",
            sha256: Some("00".repeat(32).leak()),
        };
        assert!(read_valid_cache(&wasm_path, &manifest_path, &p_bad).is_none());

        // Corrupt the blob on disk: manifest sha no longer matches → miss.
        std::fs::write(&wasm_path, b"corrupted").unwrap();
        assert!(read_valid_cache(&wasm_path, &manifest_path, &p).is_none());
    }

    /// Missing cache files → `None`, never a panic.
    #[test]
    fn read_valid_cache_missing_files_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        let p = WasmGrammarPin {
            lang_id: "fake",
            version: "0.0.0",
            blob_stem: "fake",
            sha256: None,
        };
        assert!(read_valid_cache(
            &tmp.path().join("nope.wasm"),
            &tmp.path().join("nope.json"),
            &p
        )
        .is_none());
    }

    /// Real network download + load. `#[ignore]`d so it never runs in the gate.
    ///
    /// Run manually with:
    /// ```text
    /// cargo test -p mustard-core --features wasm-grammars \
    ///   ast::wasm_acquire::tests::download_and_load_ruby_grammar -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "performs a real network download; run manually"]
    fn download_and_load_ruby_grammar() {
        let bytes = acquire_wasm_bytes("ruby").expect("ruby wasm downloads");
        assert!(bytes.len() > 1024, "grammar blob suspiciously small");
        assert_eq!(&bytes[0..4], b"\0asm", "not a wasm module header");

        let engine = Engine::default();
        let language = acquire_language(&engine, "ruby").expect("ruby loads into a Language");
        assert!(language.is_wasm());
    }
}
