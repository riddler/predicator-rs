//! Verify-then-add, RATCHET.md's rule 3: the only writer of the registry.
//!
//! Its only input is the reports of a run; there is no way to add a case by
//! id, so every line the file holds was produced by a run that observed a
//! pass. It never removes an entry: an entry the run did not observe passing
//! is a regression, and it refuses to write, naming each one. A claim it is
//! asked to write must be complete, or it refuses that too. Claims it is not
//! asked about are carried forward and re-checked. The file is re-pinned to
//! the vendored manifest's `corpus_hash` and `isa_version`.

use crate::corpus::{Corpus, SURFACES};
use crate::registry::{self, Claim, Entry, Registry};
use crate::runner::Report;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// A claim written as `surface:tier`, or `None` when `text` is not one.
pub fn parse_claim(text: &str) -> Option<Claim> {
    let (surface, tier) = text.split_once(':')?;
    if !SURFACES.contains(&surface) || tier.starts_with('0') || tier.starts_with('+') {
        return None;
    }
    let tier = tier.parse::<u64>().ok()?;
    Some(Claim {
        surface: surface.to_owned(),
        tier,
    })
}

/// The registry that follows `existing` once `reports` and `claims` are
/// added, or the problems that refuse it.
pub fn next(
    existing: &Registry,
    corpus: &Corpus,
    reports: &[Report],
    claims: &[Claim],
) -> Result<Registry, Vec<String>> {
    let mut problems: Vec<String> = reports
        .iter()
        .filter(|report| report.corpus_hash != corpus.corpus_hash)
        .map(|report| {
            format!(
                "the {} report was run against {}; the vendored manifest pins {}",
                report.surface, report.corpus_hash, corpus.corpus_hash
            )
        })
        .collect();
    let versions: BTreeSet<u64> = reports.iter().map(|report| report.isa_version).collect();
    if versions.len() > 1 {
        problems.push(format!(
            "the reports disagree about the instruction-set version claimed: {versions:?}"
        ));
    }
    if !problems.is_empty() {
        return Err(problems);
    }

    let mut candidates: BTreeMap<(String, String), Entry> = BTreeMap::new();
    for report in reports {
        for id in report.passed() {
            let member = corpus
                .surface_set(&report.surface)
                .into_iter()
                .find(|case| case.id == id)
                .map(|case| case.tier);
            match member {
                Some(tier) => {
                    let entry = Entry {
                        surface: report.surface.clone(),
                        tier,
                        case_id: id.to_owned(),
                    };
                    candidates.insert(entry.pair(), entry);
                }
                None => problems.push(format!(
                    "the {} report passes {id}, which is not in that surface's case set",
                    report.surface
                )),
            }
        }
    }
    problems.extend(
        existing
            .entries
            .iter()
            .filter(|entry| !candidates.contains_key(&entry.pair()))
            .map(|entry| format!("{} on {} no longer passes", entry.case_id, entry.surface)),
    );
    if !problems.is_empty() {
        return Err(problems);
    }

    let mut by_surface: BTreeMap<String, Claim> = existing
        .claims
        .iter()
        .map(|claim| (claim.surface.clone(), claim.clone()))
        .collect();
    for claim in claims {
        by_surface.insert(claim.surface.clone(), claim.clone());
    }
    let entered: BTreeSet<(String, String)> = candidates.keys().cloned().collect();
    let claimed = reports
        .first()
        .map_or(corpus.isa_version, |report| report.isa_version);
    for claim in by_surface.values() {
        for id in registry::missing(corpus, claim, &entered, claimed) {
            problems.push(format!(
                "the claim {}:{} wants {id}, which has no entry",
                claim.surface, claim.tier
            ));
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    Ok(Registry {
        claims: by_surface.into_values().collect(),
        corpus_hash: corpus.corpus_hash.clone(),
        entries: candidates.into_values().collect(),
        implementation: existing.implementation.clone(),
        isa_version: corpus.isa_version,
    })
}

/// Writes the registry at `path` from `reports`, adding `claims`. Answers
/// the problems that stopped it, having written nothing.
pub fn write(
    path: &Path,
    corpus: &Corpus,
    reports: &[Report],
    claims: &[Claim],
) -> Result<(), Vec<String>> {
    let bytes =
        std::fs::read(path).map_err(|error| vec![format!("{}: {error}", path.display())])?;
    let existing = registry::parse(&bytes).map_err(|problem| vec![problem])?;
    let registry = next(&existing, corpus, reports, claims)?;
    std::fs::write(path, registry::encode(&registry))
        .map_err(|error| vec![format!("{}: {error}", path.display())])
}
