//! The run over the corpus: every case of a surface's set, answered pass or
//! fail, never skipped (the reference's `conformance/README.md`, "Never
//! skip"). Its output has the shape of the reference's `schema/report.json`.

use crate::corpus::{Case, Corpus, SURFACES};

/// What one case came to.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// The surface gave what the case expects.
    Pass,
    /// It did not, for the reason given.
    Fail(String),
}

/// One case's line in a report.
#[derive(Debug, Clone, PartialEq)]
pub struct CaseResult {
    /// The case's id.
    pub id: String,
    /// Pass or fail.
    pub outcome: Outcome,
}

/// A run of one surface over every tier of the corpus.
#[derive(Debug, Clone)]
pub struct Report {
    /// The surface run.
    pub surface: String,
    /// The instruction-set version the run claims.
    pub isa_version: u64,
    /// The corpus hash the run read.
    pub corpus_hash: String,
    /// The highest tier run; tiers are cumulative.
    pub tier: u64,
    /// One result per case of the surface's set.
    pub results: Vec<CaseResult>,
}

impl Report {
    /// The ids the report passes.
    pub fn passed(&self) -> Vec<&str> {
        self.results
            .iter()
            .filter(|result| result.outcome == Outcome::Pass)
            .map(|result| result.id.as_str())
            .collect()
    }
}

/// The crate has neither an evaluator nor a compiler yet, so every case is a
/// fail naming what is missing: the honest report the never-skip rule asks
/// for. The surface code, when it exists, answers here.
fn attempt(surface: &str, case: &Case) -> CaseResult {
    CaseResult {
        id: case.id.clone(),
        outcome: Outcome::Fail(format!("{surface} not implemented")),
    }
}

/// A run of `surface` over every case of its set, in every tier.
pub fn run(corpus: &Corpus, surface: &str) -> Report {
    Report {
        surface: surface.to_owned(),
        isa_version: corpus.isa_version,
        corpus_hash: corpus.corpus_hash.clone(),
        tier: corpus.top_tier(),
        results: corpus
            .surface_set(surface)
            .into_iter()
            .map(|case| attempt(surface, case))
            .collect(),
    }
}

/// A run of both surfaces, made now.
pub fn observe(corpus: &Corpus) -> Vec<Report> {
    SURFACES
        .iter()
        .map(|surface| run(corpus, surface))
        .collect()
}
