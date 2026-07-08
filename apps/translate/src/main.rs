//! `mustard-translate` — local prompt→English MT for Mustard.
//!
//! The retrieval stack (grain rank / digest) matches ENGLISH against ENGLISH —
//! code is English-canonical. User intents arrive in any language. This sidecar
//! converts them locally, with ZERO cloud/LLM tokens:
//!
//! - `text --input "<sentence>"` → one JSON line on stdout:
//!   `{"en": "<english>", "detected": "pt"}`. Input already in English (or
//!   undetectable) passes through unchanged with `detected: "en" | "unknown"`.
//! - `batch` → same contract, one stdin line in / one JSON line out (loads the
//!   model once — the cheap path when a caller has several sentences).
//!
//! Engine: OPUS-MT Marian (`Helsinki-NLP/opus-mt-ROMANCE-en`, CC-BY-4.0 —
//! commercial use OK; NLLB/Tower are CC-BY-NC and banned) executed with candle
//! (pure Rust, CPU). Weights come from the model's safetensors conversion ref
//! (`refs/pr/4` — upstream main only ships pickle/`.ot`/`.h5`); the fast
//! tokenizer JSON comes from the Xenova mirror of the SAME checkpoint (joint
//! source/target vocab, so one tokenizer both encodes pt/es/fr and decodes en).
//! Both are fetched ONCE into a per-MACHINE cache (LOCALAPPDATA on Windows —
//! never per project, never vendored into the repo).
//!
//! Decoding is GREEDY (argmax, ties → lowest token id): same input, same model
//! ⇒ same output, byte for byte. Proven by `determinism_two_fresh_loads`.
//!
//! Fail-open contract: model missing / download failed / anything unexpected →
//! print the ORIGINAL text as the translation (`{"en": <input>, ...}`) with a
//! warning on stderr and exit 0. A missed translation degrades retrieval to
//! raw-PT quality; it must never break the search. No panics: `unwrap`/`expect`
//! only in `#[cfg(test)]`.

use std::io::BufRead;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::marian;
use clap::{Parser, Subcommand};
use lingua::{Language, LanguageDetector, LanguageDetectorBuilder};
use tokenizers::Tokenizer;

// ---------------------------------------------------------------------------
// Model coordinates — one checkpoint, two repos (weights + fast tokenizer).
// ---------------------------------------------------------------------------

/// OPUS-MT romance→English (pt/es/fr/it/ro → en). X→en models need no
/// `>>lang<<` prefix — only multi-TARGET models do.
const WEIGHTS_REPO: &str = "Helsinki-NLP/opus-mt-ROMANCE-en";
/// The safetensors conversion PR (never merged upstream; `main` has only
/// pickle/`.ot`/`.h5`, none of which candle should load). Same convention the
/// candle marian example uses for opus-mt-fr-en.
const WEIGHTS_REV: &str = "refs/pr/4";
/// Fast-tokenizer JSON mirror of the same checkpoint (tokenizers-crate format;
/// upstream only ships raw `source.spm`/`target.spm`).
const TOKENIZER_REPO: &str = "Xenova/opus-mt-ROMANCE-en";

/// Keep the encoder comfortably inside `max_position_embeddings` (512).
const MAX_SOURCE_TOKENS: usize = 384;

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(
    name = "mustard-translate",
    about = "Translate a prompt to English locally (OPUS-MT via candle; no cloud tokens)."
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Translate one sentence; prints {"en":"...","detected":"pt"} on stdout.
    Text {
        /// The sentence to translate (any language; English passes through).
        #[arg(long)]
        input: String,
        /// Hard cap on generated tokens (greedy stops at </s> well before this).
        #[arg(long, default_value_t = 192)]
        max_tokens: usize,
    },
    /// Translate every stdin line (one sentence per line, one JSON line out).
    Batch {
        #[arg(long, default_value_t = 192)]
        max_tokens: usize,
    },
}

#[derive(serde::Serialize)]
struct Out<'a> {
    en: &'a str,
    detected: &'a str,
}

fn emit(en: &str, detected: &str) {
    // Struct serialization keeps field order: {"en": ..., "detected": ...}.
    match serde_json::to_string(&Out { en, detected }) {
        Ok(line) => println!("{line}"),
        // Unreachable for plain strings; still honour the fail-open contract.
        Err(e) => eprintln!("mustard-translate: emit failed ({e})"),
    }
}

fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Text { input, max_tokens } => run_text(&input, max_tokens),
        Cmd::Batch { max_tokens } => run_batch(max_tokens),
    }
}

/// One sentence. NEVER fails the process: worst case is pass-through + stderr.
fn run_text(input: &str, max_tokens: usize) {
    let detector = build_detector();
    let detected = detect(&detector, input);
    if !needs_translation(detected) {
        emit(input, detected);
        return;
    }
    match load_guarded() {
        Err(e) => {
            eprintln!(
                "mustard-translate: model unavailable ({e:#}); passing the original text through"
            );
            emit(input, detected);
        }
        Ok(mut t) => emit_translated(&mut t, input, detected, max_tokens),
    }
}

fn run_batch(max_tokens: usize) {
    let detector = build_detector();
    let mut translator: Option<Translator> = None;
    let mut translator_failed = false;
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let input = line.trim();
        if input.is_empty() {
            continue;
        }
        let detected = detect(&detector, input);
        if !needs_translation(detected) {
            emit(input, detected);
            continue;
        }
        if translator.is_none() && !translator_failed {
            match load_guarded() {
                Ok(t) => translator = Some(t),
                Err(e) => {
                    translator_failed = true;
                    eprintln!(
                        "mustard-translate: model unavailable ({e:#}); passing originals through"
                    );
                }
            }
        }
        match translator.as_mut() {
            Some(t) => emit_translated(t, input, detected, max_tokens),
            None => emit(input, detected),
        }
    }
}

// ---------------------------------------------------------------------------
// Panic containment. The fail-open contract ("a missed translation must never
// break the search") has to survive PANICS inside the ML dependencies too —
// tokenizers 0.21 genuinely panics on some malformed tokenizer JSON instead of
// returning Err. catch_unwind converts those into the ordinary pass-through
// path; the default hook still prints the panic to stderr (honest diagnostics).
// ---------------------------------------------------------------------------

fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

fn load_guarded() -> Result<Translator> {
    match std::panic::catch_unwind(Translator::load) {
        Ok(r) => r,
        Err(p) => Err(anyhow!("internal panic during model load: {}", panic_text(&*p))),
    }
}

fn translate_guarded(t: &mut Translator, input: &str, max_tokens: usize) -> Result<String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| t.translate(input, max_tokens)))
    {
        Ok(r) => r,
        Err(p) => Err(anyhow!("internal panic during translation: {}", panic_text(&*p))),
    }
}

fn emit_translated(t: &mut Translator, input: &str, detected: &str, max_tokens: usize) {
    match translate_guarded(t, input, max_tokens) {
        Ok(en) if !en.trim().is_empty() => emit(en.trim(), detected),
        Ok(_) => {
            eprintln!("mustard-translate: empty translation; passing the original text through");
            emit(input, detected);
        }
        Err(e) => {
            eprintln!(
                "mustard-translate: translation failed ({e:#}); passing the original text through"
            );
            emit(input, detected);
        }
    }
}

// ---------------------------------------------------------------------------
// Language detection — lingua restricted to {en,pt,es,fr}. The one decision
// that matters is "English or not": pt/es/fr all route to the same ROMANCE→en
// model, so confusing them is harmless; sending English through the translator
// is the only failure mode, and lingua beats whatlang on short prompts.
// ---------------------------------------------------------------------------

fn build_detector() -> LanguageDetector {
    LanguageDetectorBuilder::from_languages(&[
        Language::English,
        Language::Portuguese,
        Language::Spanish,
        Language::French,
    ])
    .build()
}

fn detect(detector: &LanguageDetector, text: &str) -> &'static str {
    match detector.detect_language_of(text) {
        Some(Language::English) => "en",
        Some(Language::Portuguese) => "pt",
        Some(Language::Spanish) => "es",
        Some(Language::French) => "fr",
        None => "unknown",
    }
}

fn needs_translation(detected: &str) -> bool {
    matches!(detected, "pt" | "es" | "fr")
}

// ---------------------------------------------------------------------------
// Machine-level model cache (mirrors the embed sidecar's model_cache_dir):
// LOCALAPPDATA\mustard-translate\models on Windows, ~/.cache/mustard-translate
// elsewhere. Per MACHINE, never per project; weights are never vendored.
// ---------------------------------------------------------------------------

fn model_cache_dir() -> PathBuf {
    for var in ["LOCALAPPDATA", "XDG_CACHE_HOME", "HOME", "USERPROFILE"] {
        if let Ok(v) = std::env::var(var) {
            if !v.is_empty() {
                let base = PathBuf::from(v);
                return if var == "HOME" || var == "USERPROFILE" {
                    base.join(".cache").join("mustard-translate")
                } else {
                    base.join("mustard-translate").join("models")
                };
            }
        }
    }
    PathBuf::from(".mustard-translate-cache")
}

struct ModelFiles {
    config: PathBuf,
    tokenizer: PathBuf,
    weights: PathBuf,
}

/// Resolve (download once, then cache-hit) the three model files.
fn ensure_model() -> Result<ModelFiles> {
    let cache = model_cache_dir();
    let _ = std::fs::create_dir_all(&cache);
    let api = hf_hub::api::sync::ApiBuilder::new()
        .with_cache_dir(cache)
        .build()
        .context("hf-hub init")?;
    let weights_repo = api.repo(hf_hub::Repo::with_revision(
        WEIGHTS_REPO.to_string(),
        hf_hub::RepoType::Model,
        WEIGHTS_REV.to_string(),
    ));
    let config = weights_repo.get("config.json").context("config.json")?;
    let weights = weights_repo
        .get("model.safetensors")
        .context("model.safetensors")?;
    let tokenizer = api
        .model(TOKENIZER_REPO.to_string())
        .get("tokenizer.json")
        .context("tokenizer.json")?;
    Ok(ModelFiles {
        config,
        tokenizer,
        weights,
    })
}

/// Parse the HF config into candle's marian Config. HF spells the activation
/// "swish"; if this candle build only knows the "silu" spelling (same
/// function, x·σ(x)), rewrite and retry once.
fn load_config(path: &Path) -> Result<marian::Config> {
    let raw = std::fs::read_to_string(path).context("read config.json")?;
    match serde_json::from_str::<marian::Config>(&raw) {
        Ok(cfg) => Ok(cfg),
        Err(first) => {
            let mut v: serde_json::Value = serde_json::from_str(&raw)?;
            if v.get("activation_function").and_then(|a| a.as_str()) == Some("swish") {
                v["activation_function"] = "silu".into();
                if let Ok(cfg) = serde_json::from_value::<marian::Config>(v) {
                    return Ok(cfg);
                }
            }
            Err(anyhow!("config.json does not parse as a Marian config: {first}"))
        }
    }
}

/// Load the fast-tokenizer JSON. Xenova's Marian exports write a NO-OP
/// normalizer as `{"type": "Precompiled", "precompiled_charsmap": null}`;
/// the tokenizers crate panics deserializing that (it expects a base64
/// string). A null charsmap means "no normalization" — transformers.js
/// treats it as such — so drop the normalizer before handing it over.
fn load_tokenizer(path: &Path) -> Result<Tokenizer> {
    let raw = std::fs::read_to_string(path).context("read tokenizer.json")?;
    let mut v: serde_json::Value = serde_json::from_str(&raw).context("tokenizer.json parse")?;
    let null_precompiled = v.get("normalizer").is_some_and(|n| {
        n.get("type").and_then(|t| t.as_str()) == Some("Precompiled")
            && n.get("precompiled_charsmap")
                .is_none_or(serde_json::Value::is_null)
    });
    if null_precompiled {
        v["normalizer"] = serde_json::Value::Null;
    }
    Tokenizer::from_bytes(serde_json::to_vec(&v)?).map_err(|e| anyhow!("tokenizer load: {e}"))
}

// ---------------------------------------------------------------------------
// The translator — encoder once, greedy KV-cached decode.
// ---------------------------------------------------------------------------

struct Translator {
    model: marian::MTModel,
    tokenizer: Tokenizer,
    cfg: marian::Config,
    device: Device,
}

impl Translator {
    fn load() -> Result<Self> {
        let files = ensure_model()?;
        let cfg = load_config(&files.config)?;
        let tokenizer = load_tokenizer(&files.tokenizer)?;
        let device = Device::Cpu;
        // Buffered (not mmaped) load keeps the crate free of `unsafe`.
        let data = std::fs::read(&files.weights).context("read model.safetensors")?;
        let vb = VarBuilder::from_buffered_safetensors(data, DType::F32, &device)?;
        let model = marian::MTModel::new(&cfg, vb).context("build Marian model")?;
        Ok(Self {
            model,
            tokenizer,
            cfg,
            device,
        })
    }

    fn translate(&mut self, text: &str, max_tokens: usize) -> Result<String> {
        self.model.reset_kv_cache();
        let enc = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| anyhow!("encode: {e}"))?;
        let mut ids: Vec<u32> = enc.get_ids().to_vec();
        if ids.len() > MAX_SOURCE_TOKENS {
            ids.truncate(MAX_SOURCE_TOKENS - 1);
            ids.push(self.cfg.eos_token_id);
        }
        // The tokenizer's TemplateProcessing already appends </s>; keep a
        // belt-and-braces guarantee since Marian requires the terminator.
        if ids.last() != Some(&self.cfg.eos_token_id) {
            ids.push(self.cfg.eos_token_id);
        }
        let src = Tensor::new(ids.as_slice(), &self.device)?.unsqueeze(0)?;
        let encoder_xs = self.model.encoder().forward(&src, 0)?;

        let mut out: Vec<u32> = vec![self.cfg.decoder_start_token_id];
        for step in 0..max_tokens {
            let context_len = if step == 0 { out.len() } else { 1 };
            let start = out.len() - context_len;
            let xs = Tensor::new(&out[start..], &self.device)?.unsqueeze(0)?;
            let logits = self.model.decode(&xs, &encoder_xs, start)?;
            let logits = logits.squeeze(0)?;
            let last = logits.get(logits.dim(0)?.saturating_sub(1))?;
            let scores: Vec<f32> = last.to_vec1()?;
            let next = greedy_pick(&scores, self.cfg.pad_token_id)
                .ok_or_else(|| anyhow!("empty logits"))?;
            if next == self.cfg.eos_token_id {
                break;
            }
            out.push(next);
        }
        let text = self
            .tokenizer
            .decode(&out[1..], true)
            .map_err(|e| anyhow!("decode: {e}"))?;
        Ok(text.trim().to_string())
    }
}

/// Greedy argmax over the vocab. `<pad>` is masked (HF marks it bad_words);
/// NaN scores are treated as -inf; ties break to the LOWEST token id, so the
/// pick is total-order deterministic.
fn greedy_pick(scores: &[f32], pad_id: u32) -> Option<u32> {
    let mut best: Option<(usize, f32)> = None;
    for (i, &s) in scores.iter().enumerate() {
        if i as u32 == pad_id || s.is_nan() {
            continue;
        }
        match best {
            Some((_, b)) if s <= b => {}
            _ => best = Some((i, s)),
        }
    }
    best.map(|(i, _)| i as u32)
}

// ---------------------------------------------------------------------------
// Tests. The model-dependent tests use the machine cache; when it is absent
// and the download fails (offline CI), they SKIP with a note rather than fake
// a pass — the determinism proof is real whenever the model is present.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greedy_is_deterministic_and_masks_pad() {
        let scores = vec![0.1_f32, 3.0, 3.0, -1.0];
        // Tie between ids 1 and 2 → lowest id wins.
        assert_eq!(greedy_pick(&scores, 65000), Some(1));
        // Masking the winner falls to the next best.
        assert_eq!(greedy_pick(&scores, 1), Some(2));
        // NaN never wins.
        let scores = vec![f32::NAN, 0.5];
        assert_eq!(greedy_pick(&scores, 65000), Some(1));
        assert_eq!(greedy_pick(&[], 0), None);
    }

    #[test]
    fn output_shape_is_en_then_detected() {
        let s = serde_json::to_string(&Out {
            en: "x",
            detected: "en",
        })
        .unwrap();
        assert_eq!(s, r#"{"en":"x","detected":"en"}"#);
    }

    #[test]
    fn detects_short_prompts() {
        let d = build_detector();
        assert_eq!(detect(&d, "adicionar um campo de observações no formulário de contrato"), "pt");
        assert_eq!(detect(&d, "add a notes field to the contract form"), "en");
        assert_eq!(detect(&d, "onde é feita a conciliação do extrato bancário"), "pt");
    }

    #[test]
    fn english_passes_through() {
        assert!(!needs_translation("en"));
        assert!(!needs_translation("unknown"));
        assert!(needs_translation("pt"));
        assert!(needs_translation("es"));
        assert!(needs_translation("fr"));
    }

    /// GREEDY determinism across two FRESH model loads: same input ⇒ byte-equal
    /// output. Also a loose correctness smoke on the domain stress words.
    #[test]
    fn determinism_two_fresh_loads() {
        if ensure_model().is_err() {
            eprintln!("SKIP determinism_two_fresh_loads: model cache absent and download failed");
            return;
        }
        let input = "onde é feita a conciliação do extrato bancário";
        let a = {
            let mut t = Translator::load().expect("first load");
            t.translate(input, 192).expect("first translate")
        };
        let b = {
            let mut t = Translator::load().expect("second load");
            t.translate(input, 192).expect("second translate")
        };
        assert_eq!(a, b, "greedy decode must be reproducible across loads");
        assert!(
            a.to_lowercase().contains("bank"),
            "expected a bank-statement translation, got: {a}"
        );
    }
}
