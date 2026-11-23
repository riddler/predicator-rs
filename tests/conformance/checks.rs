//! The hash rule over the vendored copies: the corpus is the one
//! `conformance/SOURCE.json` says it is, each transcript file is the one
//! `transcript/VENDORED.json` says it is, and `transcript/SUPERSEDED.json`
//! names only rows that exist.
//!
//! The corpus hash is the SHA-256 of the tier files' bytes, concatenated in
//! ascending tier order, written `sha256:` and lowercase hex; it must equal
//! the manifest's `corpus_hash` and `SOURCE.json`'s, and each tier file must
//! hold its tier's `case_count` lines.
//!
//! A mismatch names the field that disagrees. It is never repaired by
//! rewriting a hash: the hash is what a claim of conformance is anchored to,
//! and a hash rewritten to match whatever is on disk anchors it to nothing.
//! The fix is to restore the copied bytes, or to vendor a new tag and
//! re-record it (`conformance/README.md`).

use crate::corpus::{Tier, manifest_tiers, read_json, sha256};
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::path::Path;

const SOURCE: &str = "SOURCE.json";
const VENDORED: &str = "transcript/VENDORED.json";
const SUPERSEDED: &str = "transcript/SUPERSEDED.json";

/// The transcripts a superseded row may name.
const TRANSCRIPTS: [&str; 2] = ["compile.json", "tokens.json"];

fn lines(bytes: &[u8]) -> usize {
    bytes.iter().filter(|byte| **byte == b'\n').count()
}

/// Every disagreement under `dir`, one sentence each; empty when the copies
/// are the ones their records name.
pub fn problems(dir: &Path) -> Vec<String> {
    let mut problems = corpus_problems(dir).unwrap_or_else(|problem| vec![problem]);
    problems.extend(transcript_problems(dir).unwrap_or_else(|problem| vec![problem]));
    if let Err(problem) = superseded(dir) {
        problems.push(problem);
    }
    problems
}

fn provenance_problems(name: &str, record: &Value) -> Vec<String> {
    let mut problems: Vec<String> = ["repo", "tag"]
        .iter()
        .filter(|key| {
            record
                .get(**key)
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
        })
        .map(|key| format!("{name} {key} is {}, not a name", field(record, key)))
        .collect();
    let commit = record
        .get("sha")
        .and_then(Value::as_str)
        .is_some_and(|sha| {
            sha.len() == 40
                && sha
                    .bytes()
                    .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        });
    if !commit {
        problems.push(format!(
            "{name} sha is {}, not a commit",
            field(record, "sha")
        ));
    }
    problems
}

fn plain(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

fn field(record: &Value, key: &str) -> Value {
    record.get(key).cloned().unwrap_or(Value::Null)
}

fn corpus_problems(dir: &Path) -> Result<Vec<String>, String> {
    let manifest = read_json(dir, "manifest.json")?;
    let source = read_json(dir, SOURCE)?;
    let tiers = manifest_tiers(&manifest)?;
    let mut problems = Vec::new();
    let mut all = Vec::new();
    for tier in &tiers {
        match std::fs::read(dir.join(&tier.file)) {
            Ok(bytes) => {
                count_problem(tier, &bytes, &mut problems);
                all.extend_from_slice(&bytes);
            }
            Err(_) => problems.push(format!(
                "{} is missing, and manifest.json lists it",
                tier.file
            )),
        }
    }
    if !problems.is_empty() {
        return Ok(problems);
    }
    let listed: BTreeSet<&str> = tiers.iter().map(|tier| tier.file.as_str()).collect();
    let present = std::fs::read_dir(dir.join("corpus"))
        .map_err(|error| format!("corpus/ cannot be read: {error}"))?;
    let mut names: Vec<String> = present
        .filter_map(Result::ok)
        .map(|entry| format!("corpus/{}", entry.file_name().to_string_lossy()))
        .collect();
    names.sort();
    problems.extend(
        names
            .iter()
            .filter(|name| !listed.contains(name.as_str()))
            .map(|name| format!("{name} is not a tier file manifest.json lists")),
    );
    let observed = Value::String(sha256(&all));
    for (name, record) in [("manifest.json", &manifest), (SOURCE, &source)] {
        let recorded = field(record, "corpus_hash");
        if recorded != observed {
            problems.push(format!(
                "{name} corpus_hash is {recorded}; the tier files hash to {observed}"
            ));
        }
    }
    problems.extend(provenance_problems(SOURCE, &source));
    let (have, want) = (
        field(&source, "isa_version"),
        field(&manifest, "isa_version"),
    );
    if have != want {
        problems.push(format!(
            "{SOURCE} isa_version is {have}; manifest.json isa_version is {want}"
        ));
    }
    Ok(problems)
}

fn count_problem(tier: &Tier, bytes: &[u8], problems: &mut Vec<String>) {
    let count = lines(bytes);
    let complete = bytes.is_empty() || bytes.ends_with(b"\n");
    if !complete || u64::try_from(count).ok() != Some(tier.case_count) {
        problems.push(format!(
            "manifest.json tier {} case_count is {}; {} holds {count} lines{}",
            tier.tier,
            tier.case_count,
            tier.file,
            if complete { "" } else { " and an unended one" }
        ));
    }
}

/// Each tier file's own digest and line count, for finding which one moved
/// when the concatenated hash disagrees (the hash alone cannot say).
pub fn digests(dir: &Path) -> Vec<String> {
    let tiers = match read_json(dir, "manifest.json").and_then(|manifest| manifest_tiers(&manifest))
    {
        Ok(tiers) => tiers,
        Err(problem) => return vec![problem],
    };
    tiers
        .iter()
        .map(|tier| match std::fs::read(dir.join(&tier.file)) {
            Ok(bytes) => format!("{} {} {} lines", tier.file, sha256(&bytes), lines(&bytes)),
            Err(_) => format!("{} is missing", tier.file),
        })
        .collect()
}

/// The transcripts copied under `transcript/` are the ones `VENDORED.json`
/// records: each file's SHA-256 equals the one it lists, every file in the
/// directory is listed (beside `VENDORED.json` and `SUPERSEDED.json`, which
/// are this repository's own), and each transcript's own record
/// (`<name>-SOURCE.json`, copied beside it) carries the same hash in its
/// `transcript_hash` and the transcript's line count in `counts.rows`. A
/// record's `corpus_hash` is not compared with the vendored corpus's: the
/// transcripts were generated from an older reference release than the
/// corpus, which is what `SUPERSEDED.json` answers for.
fn transcript_problems(dir: &Path) -> Result<Vec<String>, String> {
    let vendored = read_json(dir, VENDORED)?;
    let files = vendored
        .get("files")
        .and_then(Value::as_object)
        .ok_or(format!(
            "{VENDORED} files is not an object of name to sha256"
        ))?;
    let mut problems = provenance_problems(VENDORED, &vendored);
    let present = std::fs::read_dir(dir.join("transcript"))
        .map_err(|error| format!("transcript/ cannot be read: {error}"))?;
    let mut names: Vec<String> = present
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name != "VENDORED.json" && name != "SUPERSEDED.json")
        .collect();
    names.sort();
    problems.extend(
        names
            .iter()
            .filter(|name| !files.contains_key(name.as_str()))
            .map(|name| format!("transcript/{name} is not a file {VENDORED} lists")),
    );
    for (name, recorded) in files {
        match std::fs::read(dir.join("transcript").join(name)) {
            Err(_) => problems.push(format!(
                "transcript/{name} is missing, and {VENDORED} lists it"
            )),
            Ok(bytes) => {
                let observed = Value::String(sha256(&bytes));
                if *recorded != observed {
                    problems.push(format!(
                        "{VENDORED} files[{name}] is {recorded}; transcript/{name} hashes to {observed}"
                    ));
                }
                if let Some(stem) = name
                    .strip_suffix(".json")
                    .filter(|stem| !stem.ends_with("-SOURCE"))
                {
                    own_record_problems(dir, name, stem, files, &bytes, &mut problems)?;
                }
            }
        }
    }
    Ok(problems)
}

fn own_record_problems(
    dir: &Path,
    name: &str,
    stem: &str,
    files: &Map<String, Value>,
    bytes: &[u8],
    problems: &mut Vec<String>,
) -> Result<(), String> {
    let record_name = format!("{stem}-SOURCE.json");
    if !files.contains_key(&record_name) {
        problems.push(format!(
            "transcript/{name} has no record ({record_name}) listed beside it"
        ));
        return Ok(());
    }
    if !dir.join("transcript").join(&record_name).is_file() {
        return Ok(());
    }
    let record = read_json(dir, &format!("transcript/{record_name}"))?;
    let rows = record
        .get("counts")
        .map_or(Value::Null, |counts| field(counts, "rows"));
    let checks = [
        (
            "transcript_hash",
            field(&record, "transcript_hash"),
            Value::String(sha256(bytes)),
            "hashes to",
        ),
        ("counts.rows", rows, Value::from(lines(bytes)), "holds"),
    ];
    for (key, have, want, what) in checks {
        if have != want {
            problems.push(format!(
                "transcript/{record_name} {key} is {have}; transcript/{name} {what} {want}"
            ));
        }
    }
    Ok(())
}

/// The rows `transcript/SUPERSEDED.json` lists, per transcript, once it
/// checks: its `tag` is the vendored corpus's, and each row names a
/// transcript, a 0-based row index that exists in it, that row's own
/// `source`, and the reference's `answer` at the tag. A row is listed once.
pub fn superseded(dir: &Path) -> Result<Vec<(String, usize)>, String> {
    let record = read_json(dir, SUPERSEDED)?;
    let top = record
        .as_object()
        .ok_or(format!("{SUPERSEDED} is not a JSON object"))?;
    let keys: Vec<&str> = top.keys().map(String::as_str).collect();
    if keys != ["rows", "tag"] {
        return Err(format!(
            "{SUPERSEDED} has the keys {}; its keys are rows, tag",
            keys.join(", ")
        ));
    }
    let tag = field(&read_json(dir, SOURCE)?, "tag");
    if field(&record, "tag") != tag {
        return Err(format!(
            "{SUPERSEDED} tag is {}; {SOURCE} tag is {tag}: every answer is re-read at the vendored tag",
            field(&record, "tag")
        ));
    }
    let rows = top
        .get("rows")
        .and_then(Value::as_array)
        .ok_or(format!("{SUPERSEDED} rows is not an array"))?;
    let mut seen = BTreeSet::new();
    for row in rows {
        let item = row
            .as_object()
            .ok_or(format!("{SUPERSEDED} row {row} is not an object"))?;
        let keys: Vec<&str> = item.keys().map(String::as_str).collect();
        if keys != ["answer", "file", "index", "source"] {
            return Err(format!(
                "{SUPERSEDED} row {row} has the keys {}; a row's are answer, file, index, source",
                keys.join(", ")
            ));
        }
        let file = item.get("file").and_then(Value::as_str).unwrap_or_default();
        if !TRANSCRIPTS.contains(&file) {
            return Err(format!("{SUPERSEDED} row {row} names no transcript"));
        }
        let index = item
            .get("index")
            .and_then(Value::as_u64)
            .and_then(|index| usize::try_from(index).ok())
            .ok_or(format!("{SUPERSEDED} row {row} index is not a row index"))?;
        let text = std::fs::read_to_string(dir.join("transcript").join(file))
            .map_err(|error| format!("transcript/{file} cannot be read: {error}"))?;
        let line = text.lines().nth(index).ok_or(format!(
            "{SUPERSEDED} row {row}: transcript/{file} has no row {index}"
        ))?;
        let copied: Value = serde_json::from_str(line)
            .map_err(|error| format!("transcript/{file} row {index} is not JSON: {error}"))?;
        if field(&copied, "source") != field(row, "source") {
            return Err(format!(
                "{SUPERSEDED} row {row}: transcript/{file} row {index} has the source {}",
                field(&copied, "source")
            ));
        }
        if !seen.insert((file.to_owned(), index)) {
            return Err(format!(
                "{SUPERSEDED} lists transcript/{file} row {index} twice"
            ));
        }
    }
    Ok(seen.into_iter().collect())
}

/// The line the corpus check prints when there is nothing to report.
pub fn summary(dir: &Path) -> Result<String, String> {
    let source = read_json(dir, SOURCE)?;
    let vendored = read_json(dir, VENDORED)?;
    let manifest = read_json(dir, "manifest.json")?;
    let cases: u64 = manifest_tiers(&manifest)?
        .iter()
        .map(|tier| tier.case_count)
        .sum();
    Ok(format!(
        "corpus check: {} at {}, instruction-set version {}, {cases} cases, {}; transcripts from {} at {}",
        plain(&field(&source, "repo")),
        plain(&field(&source, "tag")),
        plain(&field(&manifest, "isa_version")),
        plain(&field(&manifest, "corpus_hash")),
        plain(&field(&vendored, "repo")),
        plain(&field(&vendored, "tag")),
    ))
}

/// The line that prints how many transcript rows the reference at the
/// vendored tag answers differently, per transcript.
pub fn superseded_summary(dir: &Path) -> Result<String, String> {
    let rows = superseded(dir)?;
    let per_file: Vec<String> = TRANSCRIPTS
        .iter()
        .map(|name| {
            format!(
                "{name} {}",
                rows.iter().filter(|(file, _)| file == name).count()
            )
        })
        .collect();
    Ok(format!(
        "superseded transcript rows: {} ({})",
        rows.len(),
        per_file.join(", ")
    ))
}
