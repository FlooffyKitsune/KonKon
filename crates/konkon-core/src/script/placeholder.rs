//! Control-code / placeholder normalizer.
//!
//! [`normalize`] replaces every recognized VN control code in a raw Japanese
//! line with a numbered `{n}` placeholder and returns a [`PlaceholderMap`] from
//! index to the literal code text. [`denormalize`] performs the inverse,
//! substituting `{n}` tokens back with their literal codes.
//!
//! Placeholders are numbered in order of a control code's *first* appearance;
//! the same literal code reused later reuses the same index. `denormalize`
//! tolerates the translated text reordering (or omitting) placeholders, but
//! errors on a `{n}` token whose index is not present in the map.

use crate::{KonkonError, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::{LazyLock, Mutex};

/// Compiled-regex cache for [`ControlPattern::Regex`] specs, keyed on the
/// pattern string (which is always `&'static str`). Avoids recompiling the same
/// small set of control-code regexes on every `normalize` / `denormalize` call.
static REGEX_CACHE: LazyLock<Mutex<HashMap<&'static str, regex::Regex>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Get (compiling once) the regex for a static pattern known to be valid.
fn cached_regex(pat: &'static str) -> regex::Regex {
    let mut cache = REGEX_CACHE.lock().expect("regex cache mutex");
    cache
        .entry(pat)
        .or_insert_with(|| regex::Regex::new(pat).expect("valid control-code regex"))
        .clone()
}

/// The `{n}` placeholder token pattern, compiled once.
static PLACEHOLDER_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\{(\d+)\}").expect("valid placeholder regex"));

/// How to recognize one class of control code.
#[derive(Debug, Clone)]
pub enum ControlPattern {
    /// Exact substring match.
    Literal(&'static str),
    /// Regex source (Phase 0 specs are static and known-valid). Compiled once
    /// and cached, keyed on this string.
    Regex(&'static str),
}

/// A named control-code recognition rule.
#[derive(Debug, Clone)]
pub struct ControlCodeSpec {
    pub name: &'static str,
    pub pattern: ControlPattern,
}

/// Map from placeholder index (as a decimal string) to the literal control-code text.
///
/// Serializes as a plain JSON object: `{ "0": "\\n", "1": "[ruby text=\"…\"]" }`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlaceholderMap(BTreeMap<String, String>);

impl PlaceholderMap {
    /// Literal code text for a placeholder index, if present.
    pub fn get(&self, k: &str) -> Option<&str> {
        self.0.get(k).map(|s| s.as_str())
    }

    /// Iterator over placeholder index keys, in ascending string order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(|s| s.as_str())
    }

    /// Number of distinct placeholders.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether there are no placeholders.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Serialize to a JSON object string.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("placeholder map serializes")
    }

    /// Parse from a JSON object string.
    pub fn from_json(s: &str) -> Result<Self> {
        Ok(serde_json::from_str(s)?)
    }
}

/// Find all `(start, end, matched_text)` spans for a single spec, in source order.
fn spans(raw: &str, spec: &ControlCodeSpec) -> Vec<(usize, usize, String)> {
    match &spec.pattern {
        ControlPattern::Literal(lit) => {
            // An empty literal would match at every position and never advance.
            if lit.is_empty() {
                return Vec::new();
            }
            let mut out = Vec::new();
            let mut from = 0;
            while let Some(rel) = raw[from..].find(lit) {
                let s = from + rel;
                out.push((s, s + lit.len(), (*lit).to_string()));
                from = s + lit.len();
            }
            out
        }
        ControlPattern::Regex(pat) => {
            let re = cached_regex(pat);
            re.find_iter(raw)
                .map(|m| (m.start(), m.end(), m.as_str().to_string()))
                .collect()
        }
    }
}

/// Replace recognized control codes in `raw` with `{n}` placeholders.
pub fn normalize(raw: &str, specs: &[ControlCodeSpec]) -> (String, PlaceholderMap) {
    // Collect every match across all specs.
    let mut hits: Vec<(usize, usize, String)> = Vec::new();
    for spec in specs {
        hits.extend(spans(raw, spec));
    }
    // Earliest start wins; on equal start, the longer match wins.
    hits.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(&a.1)));

    // Walk left-to-right, keeping a match only if it starts at or after the end
    // of the last kept match (drops anything overlapping an earlier keeper).
    let mut kept: Vec<(usize, usize, String)> = Vec::with_capacity(hits.len());
    let mut last_end = 0usize;
    for h in hits {
        if h.0 >= last_end {
            last_end = h.1;
            kept.push(h);
        }
    }

    let mut map: BTreeMap<String, String> = BTreeMap::new();
    let mut code_to_idx: BTreeMap<String, usize> = BTreeMap::new();
    let mut out = String::with_capacity(raw.len());
    let mut cursor = 0;
    for (s, e, text) in kept {
        out.push_str(&raw[cursor..s]);
        let idx = match code_to_idx.get(&text) {
            Some(&i) => i,
            None => {
                let i = code_to_idx.len();
                code_to_idx.insert(text.clone(), i);
                map.insert(i.to_string(), text);
                i
            }
        };
        out.push('{');
        out.push_str(&idx.to_string());
        out.push('}');
        cursor = e;
    }
    out.push_str(&raw[cursor..]);
    (out, PlaceholderMap(map))
}

/// Replace `{n}` placeholders in `norm` with their literal control codes.
///
/// Errors with [`KonkonError::RoundTrip`] if a `{n}` token's index is not in
/// `map` (extra placeholder), or if a key present in `map` is never referenced
/// by any `{n}` token in `norm` (missing placeholder). Reordering — every key
/// used exactly once in a different order — is allowed. The error's `Display`
/// contains the offending `{n}` token.
pub fn denormalize(norm: &str, map: &PlaceholderMap) -> Result<String> {
    let re = &*PLACEHOLDER_RE;
    let mut out = String::with_capacity(norm.len());
    let mut last = 0;
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for m in re.captures_iter(norm) {
        let whole = m.get(0).expect("group 0 always present");
        let key = &m[1];
        let repl = map.get(key).ok_or_else(|| KonkonError::RoundTrip {
            file: "<line>".into(),
            detail: format!("unknown placeholder {{{key}}}"),
        })?;
        seen.insert(key.to_string());
        out.push_str(&norm[last..whole.start()]);
        out.push_str(repl);
        last = whole.end();
    }
    out.push_str(&norm[last..]);

    let missing: Vec<String> = map
        .keys()
        .filter(|k| !seen.contains(*k))
        .map(|k| format!("{{{k}}}"))
        .collect();
    if !missing.is_empty() {
        return Err(KonkonError::RoundTrip {
            file: "<line>".into(),
            detail: format!(
                "placeholder {} missing from translated text",
                missing.join(", ")
            ),
        });
    }

    Ok(out)
}
