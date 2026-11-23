//! The conformance apparatus: the corpus vendored under `conformance/`, the
//! run over it, the corpus check, the registry check and the ratchet that
//! writes the registry.
//!
//! It lives under `tests/` because none of it is part of the crate: the
//! package ships `src/` alone, and nothing in `src/` knows the corpus exists.
//! The contract is the reference's, at the tag `conformance/SOURCE.json`
//! names: its `conformance/README.md` (the two surfaces, the cumulative
//! tiers, the never-skip rule, retired cases) and its
//! `conformance/RATCHET.md` (the registry). `conformance/README.md` here says
//! what is copied, from where, and what is this repository's own.
//!
//! Three tests at this file's top level are stages of the full gate, each run
//! on its own by a `mise.toml` task so that what it prints is shown:
//! `corpus_check` (`mise run gate:corpus`), `registry_check`
//! (`mise run gate:registry`) and, outside the gate, `ratchet`
//! (`mise run ratchet`), the one writer of `conformance/registry.json`.

mod checks;
mod corpus;
mod ratchet;
mod registry;
mod runner;

#[cfg(test)]
mod apparatus;

use corpus::{Corpus, SURFACES, root};

/// The environment variable `ratchet` reads the claims it writes from: zero
/// or more `surface:tier` words, e.g. `evaluator:2 compiler:1`.
const CLAIMS_VARIABLE: &str = "PREDICATOR_RATCHET_CLAIMS";

// The corpus check, a stage of the full gate: the vendored copies are the
// ones their records name. It prints what was checked and the count of
// transcript rows the reference at the vendored tag answers differently.
// Sabotage: with one byte of corpus/tier-1.json flipped, this test fails
// naming the corpus hash (and the apparatus tests cover each check on a copy).
#[test]
fn corpus_check() {
    let dir = root();
    let problems = checks::problems(&dir);
    assert!(
        problems.is_empty(),
        "corpus check failed:\n  {}\nper-file digests, to find what moved:\n  {}\n\
         Restore the copied bytes or vendor a tag; never rewrite a hash.",
        problems.join("\n  "),
        checks::digests(&dir).join("\n  ")
    );
    println!("{}", checks::summary(&dir).unwrap());
    println!("{}", checks::superseded_summary(&dir).unwrap());
}

// The registry check, a stage of the full gate: every check RATCHET.md puts
// over a registry, against a run of both surfaces made now. It prints, per
// surface, how many cases of that surface's set have no entry yet.
// Sabotage: with an entry for a case no run passes added to registry.json,
// this test fails naming the entry.
#[test]
fn registry_check() {
    let dir = root();
    let corpus = Corpus::load(&dir).unwrap();
    let reports = runner::observe(&corpus);
    let bytes = std::fs::read(dir.join(registry::FILE)).unwrap();
    let problems = registry::check(&bytes, &corpus, &reports);
    assert!(
        problems.is_empty(),
        "registry check failed:\n  {}",
        problems.join("\n  ")
    );
    let parsed = registry::parse(&bytes).unwrap();
    for surface in SURFACES {
        println!(
            "registry: {surface}: {} of {} cases not yet claimed (no entry)",
            registry::unentered(&corpus, surface, &parsed).len(),
            corpus.surface_set(surface).len()
        );
    }
}

// Verify-then-add, the one writer of conformance/registry.json: it runs both
// surfaces now and adds every observed pass, with the claims the
// environment variable names. It writes a tracked file, so the suite never
// runs it; `mise run ratchet` does.
// Sabotage: none - this is the writer, not a check; the apparatus tests
// cover its refusals.
#[test]
#[ignore = "writes conformance/registry.json; run it through `mise run ratchet`"]
fn ratchet() {
    let text = std::env::var(CLAIMS_VARIABLE).unwrap_or_default();
    let claims: Vec<registry::Claim> = text
        .split_whitespace()
        .map(|word| {
            ratchet::parse_claim(word)
                .unwrap_or_else(|| panic!("ratchet: {word} is not surface:tier"))
        })
        .collect();
    let dir = root();
    let corpus = Corpus::load(&dir).unwrap();
    let path = dir.join(registry::FILE);
    if let Err(problems) = ratchet::write(&path, &corpus, &runner::observe(&corpus), &claims) {
        panic!(
            "ratchet refused; nothing was written:\n  {}",
            problems.join("\n  ")
        );
    }
    println!("ratchet: wrote conformance/{}", registry::FILE);
}
