//! Reading the vendored corpus: its manifest and every case of every tier the
//! manifest lists, in ascending tier order, each case held to the shape the
//! vendored `schema/corpus.json` allows.

use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

/// The two surfaces a case is run on, in codepoint order.
pub const SURFACES: [&str; 2] = ["compiler", "evaluator"];

/// The feature tag a case using a retired opcode carries.
const RETIRED: &str = "retired";

/// The directory the vendored corpus lives in.
pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("conformance")
}

/// The JSON file at `relative` under `dir`, parsed.
pub fn read_json(dir: &Path, relative: &str) -> Result<Value, String> {
    let bytes = std::fs::read(dir.join(relative))
        .map_err(|error| format!("{relative} cannot be read: {error}"))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("{relative} is not JSON: {error}"))
}

/// The SHA-256 of `bytes`, written `sha256:` and lowercase hex.
pub fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let mut text = String::from("sha256:");
    for byte in digest.iter() {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

/// One entry of the manifest's `tiers`.
#[derive(Debug, Clone)]
pub struct Tier {
    /// The tier's number.
    pub tier: u64,
    /// The tier file, relative to the corpus directory.
    pub file: String,
    /// How many cases the manifest says the file holds.
    pub case_count: u64,
}

/// The manifest's tiers, sorted by tier number.
pub fn manifest_tiers(manifest: &Value) -> Result<Vec<Tier>, String> {
    let listed = manifest
        .get("tiers")
        .and_then(Value::as_array)
        .ok_or("manifest.json has no tiers array")?;
    let mut tiers = listed
        .iter()
        .map(|item| {
            let field = |key: &str| item.get(key).cloned().unwrap_or(Value::Null);
            match (
                field("tier").as_u64(),
                field("file"),
                field("case_count").as_u64(),
            ) {
                (Some(tier), Value::String(file), Some(case_count)) => Ok(Tier {
                    tier,
                    file,
                    case_count,
                }),
                _ => Err(format!(
                    "manifest.json lists a tier without a tier, file and case_count: {item}"
                )),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    tiers.sort_by_key(|tier| tier.tier);
    Ok(tiers)
}

/// The keys a case of a tier file may carry, read out of the vendored
/// `schema/corpus.json` rather than written here: the keys its `properties`
/// names, the keys it requires, and its `oneOf` (exactly one of
/// `expected_result` and `expected_error`). A case outside that shape is
/// refused rather than half-read, so a corpus that grows a key is noticed the
/// day it is vendored.
#[derive(Debug)]
pub struct CaseShape {
    known: Vec<String>,
    required: Vec<String>,
    alternatives: Vec<Vec<String>>,
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

impl CaseShape {
    /// The shape `schema` (a parsed `schema/corpus.json`) gives a case.
    pub fn from_schema(schema: &Value) -> Result<CaseShape, String> {
        let known = schema
            .get("properties")
            .and_then(Value::as_object)
            .ok_or("schema/corpus.json has no properties object")?
            .keys()
            .cloned()
            .collect();
        let alternatives = schema
            .get("oneOf")
            .and_then(Value::as_array)
            .map(|branches| {
                branches
                    .iter()
                    .map(|branch| strings(branch.get("required")))
                    .collect()
            })
            .unwrap_or_default();
        Ok(CaseShape {
            known,
            required: strings(schema.get("required")),
            alternatives,
        })
    }

    /// What is wrong with `item`'s keys, one sentence each; empty when
    /// nothing is.
    pub fn problems(&self, item: &Map<String, Value>) -> Vec<String> {
        let id = item
            .get("id")
            .map_or_else(|| "null".to_owned(), Value::to_string);
        let mut problems: Vec<String> = item
            .keys()
            .filter(|key| !self.known.contains(key))
            .map(|key| {
                format!("case {id} carries the key {key:?}, which schema/corpus.json does not name")
            })
            .collect();
        problems.extend(
            self.required
                .iter()
                .filter(|key| !item.contains_key(key.as_str()))
                .map(|key| {
                    format!("case {id} lacks the key {key:?}, which schema/corpus.json requires")
                }),
        );
        let matched = self
            .alternatives
            .iter()
            .filter(|keys| keys.iter().all(|key| item.contains_key(key.as_str())))
            .count();
        if !self.alternatives.is_empty() && matched != 1 {
            problems.push(format!(
                "case {id} carries {matched} of the schema's oneOf branches; schema/corpus.json wants exactly one"
            ));
        }
        problems
    }
}

/// What a case expects of the evaluator.
#[derive(Debug, Clone, PartialEq)]
pub enum Expected {
    /// The evaluation's result, decoded from the corpus's tagged encoding.
    Result(predicator_rs::Value),
    /// The error's normative type and reason (never its message).
    Error {
        /// One of the reference's three error type names.
        kind: String,
        /// The structured reason token.
        reason: String,
    },
}

/// One case of a tier file.
#[derive(Debug, Clone)]
pub struct Case {
    /// The case's stable id.
    pub id: String,
    /// The source the compiler surface compiles; `None` for an
    /// evaluator-only case.
    pub source: Option<String>,
    /// The compiled instruction list, its literal operands decoded.
    pub instructions: predicator_rs::Value,
    /// The variable bindings, decoded.
    pub context: predicator_rs::Value,
    /// The result or error the evaluator gives.
    pub expected: Expected,
    /// The case's tier.
    pub tier: u64,
    /// The case's feature tags.
    pub features: Vec<String>,
    /// The case's free-text notes, when it has any.
    pub notes: Option<String>,
}

/// The one place a predicator value is read out of a case: the context, the
/// instructions and the expected result all pass through here, read by the
/// crate's own tagged codec (`predicator_rs::tagged::decode`) from their
/// compact JSON text.
pub fn case_value(value: &Value) -> Result<predicator_rs::Value, String> {
    predicator_rs::tagged::decode(&value.to_string())
        .map_err(|error| format!("{}: {}", error.reason(), error.message()))
}

fn read_case(line: &str, file: &str, number: usize, shape: &CaseShape) -> Result<Case, String> {
    let item: Map<String, Value> = serde_json::from_str(line)
        .map_err(|error| format!("{file} line {number} is not a JSON object: {error}"))?;
    let problems = shape.problems(&item);
    if !problems.is_empty() {
        return Err(format!("{file} line {number}: {}", problems.join("; ")));
    }
    let wrong = |what: &str| format!("{file} line {number}: {what}");
    let id = item
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| wrong("id is not a string"))?
        .to_owned();
    let source = match item.get("source") {
        Some(Value::String(text)) => Some(text.clone()),
        Some(Value::Null) => None,
        _ => return Err(wrong("source is neither a string nor null")),
    };
    let expected = match (item.get("expected_result"), item.get("expected_error")) {
        (Some(result), None) => Expected::Result(
            case_value(result).map_err(|error| wrong(&format!("expected_result: {error}")))?,
        ),
        (None, Some(error)) => {
            let field = |key: &str| error.get(key).and_then(Value::as_str).map(str::to_owned);
            match (field("type"), field("reason")) {
                (Some(kind), Some(reason)) => Expected::Error { kind, reason },
                _ => return Err(wrong("expected_error lacks a type or a reason")),
            }
        }
        _ => {
            return Err(wrong(
                "carries neither or both of expected_result and expected_error",
            ));
        }
    };
    let tier = item
        .get("tier")
        .and_then(Value::as_u64)
        .ok_or_else(|| wrong("tier is not a positive integer"))?;
    Ok(Case {
        id,
        source,
        instructions: case_value(item.get("instructions").unwrap_or(&Value::Null))
            .map_err(|error| wrong(&format!("instructions: {error}")))?,
        context: case_value(item.get("context").unwrap_or(&Value::Null))
            .map_err(|error| wrong(&format!("context: {error}")))?,
        expected,
        tier,
        features: strings(item.get("features")),
        notes: item.get("notes").and_then(Value::as_str).map(str::to_owned),
    })
}

/// The vendored corpus: its manifest's pin and every case of every tier the
/// manifest lists, in ascending tier order.
#[derive(Debug)]
pub struct Corpus {
    /// The manifest's `corpus_hash`.
    pub corpus_hash: String,
    /// The manifest's `isa_version`.
    pub isa_version: u64,
    /// Every case, tier by tier.
    pub cases: Vec<Case>,
}

impl Corpus {
    /// Reads the corpus under `dir`, refusing a line that is not JSON, a case
    /// outside the schema's shape, a case whose tier is not its file's and
    /// an id seen twice.
    pub fn load(dir: &Path) -> Result<Corpus, String> {
        let manifest = read_json(dir, "manifest.json")?;
        let shape = CaseShape::from_schema(&read_json(dir, "schema/corpus.json")?)?;
        let mut cases = Vec::new();
        for tier in manifest_tiers(&manifest)? {
            let text = std::fs::read_to_string(dir.join(&tier.file))
                .map_err(|error| format!("{} cannot be read: {error}", tier.file))?;
            for (index, line) in text.lines().enumerate() {
                if line.is_empty() {
                    continue;
                }
                let case = read_case(line, &tier.file, index + 1, &shape)?;
                if case.tier != tier.tier {
                    return Err(format!(
                        "{} line {}: case {} says tier {}, in the file of tier {}",
                        tier.file,
                        index + 1,
                        case.id,
                        case.tier,
                        tier.tier
                    ));
                }
                if cases.iter().any(|seen: &Case| seen.id == case.id) {
                    return Err(format!(
                        "{} line {}: case {} is in the corpus twice",
                        tier.file,
                        index + 1,
                        case.id
                    ));
                }
                cases.push(case);
            }
        }
        let corpus_hash = manifest
            .get("corpus_hash")
            .and_then(Value::as_str)
            .ok_or("manifest.json has no corpus_hash")?
            .to_owned();
        let isa_version = manifest
            .get("isa_version")
            .and_then(Value::as_u64)
            .ok_or("manifest.json has no isa_version")?;
        Ok(Corpus {
            corpus_hash,
            isa_version,
            cases,
        })
    }

    /// Every case for the evaluator; the cases with a source for the
    /// compiler. A case with a null source is absent from the compiler's
    /// set, not skipped by it. Any other surface has no cases.
    pub fn surface_set(&self, surface: &str) -> Vec<&Case> {
        match surface {
            "evaluator" => self.cases.iter().collect(),
            "compiler" => self
                .cases
                .iter()
                .filter(|case| case.source.is_some())
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The cases of `surface` a run claiming the instruction-set version
    /// `claimed` is answerable for: a case tagged `retired` uses an opcode
    /// retired at or below the corpus's version, so a claim of that version
    /// does not cover it, and a claim of an earlier one does. It stays a
    /// member of the evaluator's set either way.
    pub fn covered(&self, surface: &str, claimed: u64) -> Vec<&Case> {
        self.surface_set(surface)
            .into_iter()
            .filter(|case| {
                claimed < self.isa_version || !case.features.iter().any(|tag| tag == RETIRED)
            })
            .collect()
    }

    /// The tier the corpus puts the case `id` in.
    pub fn tier_of(&self, id: &str) -> Option<u64> {
        self.cases
            .iter()
            .find(|case| case.id == id)
            .map(|case| case.tier)
    }

    /// The highest tier the corpus holds.
    pub fn top_tier(&self) -> u64 {
        self.cases.iter().map(|case| case.tier).max().unwrap_or(0)
    }
}
