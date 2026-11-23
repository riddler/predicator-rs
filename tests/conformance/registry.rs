//! The file `conformance/registry.json`: the record of which cases this
//! crate passes, on which surface, against which corpus. Its contract is the
//! reference's `conformance/RATCHET.md` at the tag `SOURCE.json` names: the
//! fields, the order, the one-element-per-line encoding, verify-then-add as
//! the only writer (`ratchet`), and the checks (`check`).

use crate::corpus::{Corpus, SURFACES};
use crate::runner::Report;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// The registry's file name under the corpus directory.
pub const FILE: &str = "registry.json";

/// The top-level keys, in the codepoint order the encoding writes them.
const KEYS: [&str; 5] = [
    "claims",
    "corpus_hash",
    "entries",
    "implementation",
    "isa_version",
];

/// One verified pass.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Entry {
    // Field order is RATCHET.md's sort order: surface, then tier, then
    // case id, each ascending (String's order is by byte, which for UTF-8
    // is codepoint order).
    /// The surface it passed on.
    pub surface: String,
    /// The case's tier, copied from the corpus.
    pub tier: u64,
    /// The case.
    pub case_id: String,
}

impl Entry {
    /// The pair an entry is matched on: never the id alone.
    pub fn pair(&self) -> (String, String) {
        (self.surface.clone(), self.case_id.clone())
    }

    fn describe(&self) -> String {
        format!("{} on {}", self.case_id, self.surface)
    }
}

/// "Every case in tiers 1 through `tier` on `surface` has an entry."
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    /// The surface claimed.
    pub surface: String,
    /// The highest tier claimed.
    pub tier: u64,
}

/// A parsed registry.
#[derive(Debug, Clone, PartialEq)]
pub struct Registry {
    /// The claims, at most one per surface.
    pub claims: Vec<Claim>,
    /// The pin.
    pub corpus_hash: String,
    /// The ratchet itself.
    pub entries: Vec<Entry>,
    /// The implementation's own name, diagnostic only.
    pub implementation: String,
    /// The manifest's instruction-set version at pin time.
    pub isa_version: u64,
}

fn object<'a>(value: &'a Value, what: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{what} is not a JSON object"))
}

fn exact_keys(item: &Map<String, Value>, keys: &[&str], what: &str) -> Result<(), String> {
    let have: Vec<&str> = item.keys().map(String::as_str).collect();
    if have == keys {
        Ok(())
    } else {
        Err(format!(
            "{what} has the keys {}; RATCHET.md's are {}",
            have.join(", "),
            keys.join(", ")
        ))
    }
}

fn text(item: &Map<String, Value>, key: &str, what: &str) -> Result<String, String> {
    item.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("{what} {key} is not a string"))
}

fn tier(item: &Map<String, Value>, key: &str, what: &str) -> Result<u64, String> {
    item.get(key)
        .and_then(Value::as_u64)
        .filter(|number| *number >= 1)
        .ok_or_else(|| format!("{what} {key} is not an integer of at least 1"))
}

fn array<'a>(item: &'a Map<String, Value>, key: &str) -> Result<&'a Vec<Value>, String> {
    item.get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("the registry's {key} is not an array"))
}

/// The registry `bytes` hold, or a sentence saying why they are not one.
pub fn parse(bytes: &[u8]) -> Result<Registry, String> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|error| format!("the registry is not JSON: {error}"))?;
    let top = object(&value, "the registry")?;
    exact_keys(top, &KEYS, "the registry")?;
    let claims = array(top, "claims")?
        .iter()
        .map(|item| {
            let claim = object(item, "a claim")?;
            exact_keys(claim, &["surface", "tier"], "a claim")?;
            Ok(Claim {
                surface: text(claim, "surface", "a claim's")?,
                tier: tier(claim, "tier", "a claim's")?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let entries = array(top, "entries")?
        .iter()
        .map(|item| {
            let entry = object(item, "an entry")?;
            exact_keys(entry, &["case_id", "surface", "tier"], "an entry")?;
            Ok(Entry {
                case_id: text(entry, "case_id", "an entry's")?,
                surface: text(entry, "surface", "an entry's")?,
                tier: tier(entry, "tier", "an entry's")?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Registry {
        claims,
        corpus_hash: text(top, "corpus_hash", "the registry's")?,
        entries,
        implementation: text(top, "implementation", "the registry's")?,
        isa_version: top
            .get("isa_version")
            .and_then(Value::as_u64)
            .ok_or("the registry's isa_version is not an integer")?,
    })
}

fn quoted(text: &str) -> String {
    Value::String(text.to_owned()).to_string()
}

fn block(name: &str, lines: Vec<String>) -> String {
    if lines.is_empty() {
        format!("{}: [\n]", quoted(name))
    } else {
        format!("{}: [\n{}\n]", quoted(name), lines.join(",\n"))
    }
}

/// The bytes `registry` is required to have on disk (RATCHET.md, rule 2):
/// top-level keys in codepoint order, no indentation, each claim and each
/// entry on one line as canonical JSON, one trailing newline. It sorts
/// rather than trusting the order it was handed, so a re-encode comparison
/// catches a reordered file as well as a reflowed one. The one shape
/// RATCHET.md's example does not show, an empty array, keeps its brackets on
/// their own lines with nothing between them.
pub fn encode(registry: &Registry) -> String {
    let mut claims = registry.claims.clone();
    claims.sort_by(|left, right| left.surface.cmp(&right.surface));
    let mut entries = registry.entries.clone();
    entries.sort();
    let claim_lines = claims
        .iter()
        .map(|claim| {
            format!(
                "{{\"surface\":{},\"tier\":{}}}",
                quoted(&claim.surface),
                claim.tier
            )
        })
        .collect();
    let entry_lines = entries
        .iter()
        .map(|entry| {
            format!(
                "{{\"case_id\":{},\"surface\":{},\"tier\":{}}}",
                quoted(&entry.case_id),
                quoted(&entry.surface),
                entry.tier
            )
        })
        .collect();
    format!(
        "{{\n{},\n\"corpus_hash\": {},\n{},\n\"implementation\": {},\n\"isa_version\": {}\n}}\n",
        block("claims", claim_lines),
        quoted(&registry.corpus_hash),
        block("entries", entry_lines),
        quoted(&registry.implementation),
        registry.isa_version
    )
}

/// The ids a claim covers that have no entry in `entered` (a set of pairs):
/// every case of the claim's surface in tiers 1 through its tier, of those a
/// run claiming the instruction-set version `claimed` is answerable for.
pub fn missing(
    corpus: &Corpus,
    claim: &Claim,
    entered: &BTreeSet<(String, String)>,
    claimed: u64,
) -> Vec<String> {
    corpus
        .covered(&claim.surface, claimed)
        .into_iter()
        .filter(|case| case.tier <= claim.tier)
        .filter(|case| !entered.contains(&(claim.surface.clone(), case.id.clone())))
        .map(|case| case.id.clone())
        .collect()
}

/// The ids of `surface`'s case set that `registry` has no entry for: what
/// the run reports per surface as not yet claimed.
pub fn unentered(corpus: &Corpus, surface: &str, registry: &Registry) -> Vec<String> {
    let entered: BTreeSet<&str> = registry
        .entries
        .iter()
        .filter(|entry| entry.surface == surface)
        .map(|entry| entry.case_id.as_str())
        .collect();
    corpus
        .surface_set(surface)
        .into_iter()
        .filter(|case| !entered.contains(case.id.as_str()))
        .map(|case| case.id.clone())
        .collect()
}

/// One sentence per problem with the registry in `bytes`, against `corpus`
/// and `reports` of a run made now; empty when it passes. The checks are
/// RATCHET.md's: the pin (its `corpus_hash` and `isa_version` are the
/// vendored manifest's), the order, at most one entry per pair and one claim
/// per surface, membership and its tier sibling, the encoding (re-encoding
/// reproduces the bytes, which is how a hand edit is refused), currency
/// (every entry passes in the run made now) and completeness (a claim of
/// tier N has an entry for every case it covers).
pub fn check(bytes: &[u8], corpus: &Corpus, reports: &[Report]) -> Vec<String> {
    let registry = match parse(bytes) {
        Ok(registry) => registry,
        Err(problem) => return vec![problem],
    };
    let mut problems = Vec::new();
    pin(&registry, corpus, &mut problems);
    order(&registry, &mut problems);
    duplicates(&registry, &mut problems);
    membership(&registry, corpus, &mut problems);
    if encode(&registry).as_bytes() != bytes {
        problems.push(
            "the registry's bytes are not what re-encoding it produces; something other than the ratchet wrote it"
                .to_owned(),
        );
    }
    currency(&registry, reports, &mut problems);
    let entered: BTreeSet<(String, String)> = registry.entries.iter().map(Entry::pair).collect();
    for claim in &registry.claims {
        if !SURFACES.contains(&claim.surface.as_str()) {
            problems.push(format!("the claim on {} names no surface", claim.surface));
            continue;
        }
        let claimed = reports
            .first()
            .map_or(corpus.isa_version, |report| report.isa_version);
        for id in missing(corpus, claim, &entered, claimed) {
            problems.push(format!(
                "the claim {}:{} wants {id}, which has no entry",
                claim.surface, claim.tier
            ));
        }
    }
    problems
}

fn pin(registry: &Registry, corpus: &Corpus, problems: &mut Vec<String>) {
    if registry.corpus_hash != corpus.corpus_hash {
        problems.push(format!(
            "the registry pins {}; the vendored manifest's corpus_hash is {}",
            registry.corpus_hash, corpus.corpus_hash
        ));
    }
    if registry.isa_version != corpus.isa_version {
        problems.push(format!(
            "the registry's isa_version is {}; the vendored manifest's is {}",
            registry.isa_version, corpus.isa_version
        ));
    }
}

fn order(registry: &Registry, problems: &mut Vec<String>) {
    for pair in registry.entries.windows(2) {
        if let [left, right] = pair {
            if left > right {
                problems.push(format!(
                    "the entries are not sorted: {} comes before {}",
                    left.describe(),
                    right.describe()
                ));
            }
        }
    }
    let surfaces: Vec<&str> = registry
        .claims
        .iter()
        .map(|claim| claim.surface.as_str())
        .collect();
    if !surfaces.is_sorted() {
        problems.push("the claims are not sorted by surface".to_owned());
    }
}

fn duplicates(registry: &Registry, problems: &mut Vec<String>) {
    let mut seen = BTreeMap::new();
    for entry in &registry.entries {
        *seen.entry(entry.pair()).or_insert(0) += 1;
    }
    for ((surface, id), count) in seen {
        if count > 1 {
            problems.push(format!("{id} on {surface} is entered twice"));
        }
    }
    let mut claimed = BTreeMap::new();
    for claim in &registry.claims {
        *claimed.entry(claim.surface.as_str()).or_insert(0) += 1;
    }
    for (surface, count) in claimed {
        if count > 1 {
            problems.push(format!("the {surface} surface is claimed twice"));
        }
    }
}

fn membership(registry: &Registry, corpus: &Corpus, problems: &mut Vec<String>) {
    for entry in &registry.entries {
        if !SURFACES.contains(&entry.surface.as_str()) {
            problems.push(format!("{} names no surface", entry.describe()));
            continue;
        }
        let member = corpus
            .surface_set(&entry.surface)
            .iter()
            .any(|case| case.id == entry.case_id);
        if !member {
            problems.push(format!(
                "{} is not in the {} surface's case set",
                entry.describe(),
                entry.surface
            ));
            continue;
        }
        if let Some(tier) = corpus.tier_of(&entry.case_id) {
            if tier != entry.tier {
                problems.push(format!(
                    "{} says tier {}; the corpus puts it in tier {tier}",
                    entry.describe(),
                    entry.tier
                ));
            }
        }
    }
}

fn currency(registry: &Registry, reports: &[Report], problems: &mut Vec<String>) {
    for entry in &registry.entries {
        match reports
            .iter()
            .find(|report| report.surface == entry.surface)
        {
            None => problems.push(format!(
                "nothing ran the {} surface, and {} is entered",
                entry.surface,
                entry.describe()
            )),
            Some(report) => {
                if !report.passed().contains(&entry.case_id.as_str()) {
                    problems.push(format!("{} no longer passes", entry.describe()));
                }
            }
        }
    }
}
