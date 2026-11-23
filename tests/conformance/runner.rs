//! The run over the corpus: every case of a surface's set, answered pass or
//! fail, never skipped (the reference's `conformance/README.md`, "Never
//! skip"). Its output has the shape of the reference's `schema/report.json`.

use crate::corpus::{Case, Corpus, Expected, SURFACES};
use predicator_rs::{Context, Instructions, Options, Value};

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

/// What a case comes to on `surface`. The evaluator surface runs the case's
/// instruction list over its context with the default options and compares
/// the answer (by strict equality) or the error's type and reason (never its
/// message) with what the case expects. The crate has no compiler yet, so
/// every compiler case is a fail naming the missing surface: the honest
/// report the never-skip rule asks for.
fn attempt(surface: &str, case: &Case) -> CaseResult {
    let outcome = match surface {
        "evaluator" => evaluate(case),
        _ => Outcome::Fail(format!("{surface} not implemented")),
    };
    CaseResult {
        id: case.id.clone(),
        outcome,
    }
}

fn evaluate(case: &Case) -> Outcome {
    let Value::Map(bindings) = &case.context else {
        return Outcome::Fail("the case's context is not a map".to_owned());
    };
    let answer = Instructions::from_json(&case.instructions_json).and_then(|instructions| {
        predicator_rs::evaluate(
            &instructions,
            &Context::from(bindings.clone()),
            &Options::new(),
        )
    });
    match (&case.expected, answer) {
        (Expected::Result(want), Ok(got)) if *want == got => Outcome::Pass,
        (Expected::Error { kind, reason }, Err(error))
            if error.type_name() == kind && error.reason() == reason =>
        {
            Outcome::Pass
        }
        (want, got) => Outcome::Fail(format!("expected {want:?}, got {got:?}")),
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
