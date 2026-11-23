//! Tests of the apparatus itself: each check run over a copy of the vendored
//! corpus that has been broken in one way, so a check that cannot fail is
//! noticed. Copies live under the build's own temporary directory.

use crate::checks;
use crate::corpus::{Corpus, Expected, root};
use crate::ratchet;
use crate::registry::{self, Claim, Entry, Registry};
use crate::runner::{self, CaseResult, Outcome, Report};
use predicator_rs::Value;
use predicator_rs::tagged::{decode, encode};
use std::path::{Path, PathBuf};

const CORPUS_HASH: &str = "sha256:2eebecf5b5493aaed4ddfd5ac7926ef05211c4d57b6c27eda7724320486b7686";

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// A fresh copy of the vendored corpus directory, named for the test.
fn copy(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("conformance-{name}"));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    copy_dir(&root(), &dir);
    dir
}

fn edit(path: &Path, change: impl FnOnce(String) -> String) {
    let text = std::fs::read_to_string(path).unwrap();
    std::fs::write(path, change(text)).unwrap();
}

fn mentions(problems: &[String], text: &str) -> bool {
    problems.iter().any(|problem| problem.contains(text))
}

fn corpus() -> Corpus {
    Corpus::load(&root()).unwrap()
}

/// A run in which exactly `passes` pass on `surface` and everything else,
/// on both surfaces, fails, whatever the crate passes today.
fn reports_passing(corpus: &Corpus, surface: &str, passes: &[&str]) -> Vec<Report> {
    runner::observe(corpus)
        .into_iter()
        .map(|mut report| {
            let on_surface = report.surface == surface;
            for result in &mut report.results {
                result.outcome = if on_surface && passes.contains(&result.id.as_str()) {
                    Outcome::Pass
                } else {
                    Outcome::Fail("not passed in this run".to_owned())
                };
            }
            report
        })
        .collect()
}

fn empty_registry(corpus: &Corpus) -> Registry {
    Registry {
        claims: Vec::new(),
        corpus_hash: corpus.corpus_hash.clone(),
        entries: Vec::new(),
        implementation: "predicator-rs".to_owned(),
        isa_version: corpus.isa_version,
    }
}

fn entry(surface: &str, tier: u64, case_id: &str) -> Entry {
    Entry {
        surface: surface.to_owned(),
        tier,
        case_id: case_id.to_owned(),
    }
}

// Sabotage: with checks::superseded_summary's total off by one, this test
// fails.
#[test]
fn the_vendored_copies_match_their_records() {
    let dir = root();
    assert_eq!(checks::problems(&dir), Vec::<String>::new());
    let summary = checks::summary(&dir).unwrap();
    assert!(summary.contains(CORPUS_HASH), "{summary}");
    assert!(summary.contains("475 cases"), "{summary}");
    assert!(
        summary.contains("riddler/predicator-ex at v9.6.0"),
        "{summary}"
    );
    assert!(
        summary.contains("riddler/predicator-ts at v0.7.0"),
        "{summary}"
    );
    assert_eq!(
        checks::superseded_summary(&dir).unwrap(),
        "superseded transcript rows: 0 (compile.json 0, tokens.json 0)"
    );
}

// Sabotage: with the corpus_hash comparison in checks::corpus_problems
// skipped, this test fails.
#[test]
fn one_flipped_corpus_byte_fails_the_corpus_hash() {
    let dir = copy("flipped-byte");
    edit(&dir.join("corpus/tier-3.json"), |text| {
        text.replacen("\"tier\":3", "\"tier\":4", 1)
    });
    let problems = checks::problems(&dir);
    assert!(
        mentions(&problems, "manifest.json corpus_hash is"),
        "{problems:?}"
    );
    assert!(
        mentions(&problems, "SOURCE.json corpus_hash is"),
        "{problems:?}"
    );
    assert!(
        checks::digests(&dir)
            .iter()
            .any(|line| line.starts_with("corpus/tier-3.json sha256:")),
    );
}

// Sabotage: with count_problem's comparison against case_count dropped, this
// test fails.
#[test]
fn a_dropped_case_fails_its_tier_count() {
    let dir = copy("dropped-case");
    edit(&dir.join("corpus/tier-9.json"), |text| {
        let mut lines: Vec<&str> = text.lines().collect();
        lines.pop();
        format!("{}\n", lines.join("\n"))
    });
    let problems = checks::problems(&dir);
    assert!(
        mentions(
            &problems,
            "tier 9 case_count is 6; corpus/tier-9.json holds 5 lines"
        ),
        "{problems:?}"
    );
}

// Sabotage: with the unlisted-file filter in checks::corpus_problems
// removed, this test fails.
#[test]
fn a_tier_file_the_manifest_does_not_list_fails() {
    let dir = copy("unlisted-tier");
    std::fs::write(dir.join("corpus/tier-11.json"), "").unwrap();
    let problems = checks::problems(&dir);
    assert!(
        mentions(
            &problems,
            "corpus/tier-11.json is not a tier file manifest.json lists"
        ),
        "{problems:?}"
    );
}

// Sabotage: with the recorded-hash comparison in checks::transcript_problems
// skipped, this test fails.
#[test]
fn one_flipped_transcript_byte_fails_its_recorded_hash() {
    let dir = copy("flipped-transcript");
    edit(&dir.join("transcript/tokens.json"), |text| {
        text.replacen("limit > 85", "limit > 86", 1)
    });
    let problems = checks::problems(&dir);
    assert!(
        mentions(&problems, "transcript/VENDORED.json files[tokens.json] is"),
        "{problems:?}"
    );
    assert!(
        mentions(
            &problems,
            "transcript/tokens-SOURCE.json transcript_hash is"
        ),
        "{problems:?}"
    );
}

// Sabotage: with the counts.rows check in checks::own_record_problems
// dropped, this test fails.
#[test]
fn a_transcript_row_count_that_disagrees_with_its_record_fails() {
    let dir = copy("transcript-rows");
    edit(&dir.join("transcript/compile-SOURCE.json"), |text| {
        text.replacen("\"rows\": 199", "\"rows\": 198", 1)
    });
    let problems = checks::problems(&dir);
    assert!(
        mentions(
            &problems,
            "transcript/compile-SOURCE.json counts.rows is 198"
        ),
        "{problems:?}"
    );
}

// Sabotage: with the unknown-key filter in CaseShape::problems removed, this
// test fails.
#[test]
fn a_case_carrying_a_key_the_schema_does_not_name_is_refused() {
    let dir = copy("unknown-key");
    edit(&dir.join("corpus/tier-1.json"), |text| {
        text.replacen("{\"context\"", "{\"colour\":1,\"context\"", 1)
    });
    let error = Corpus::load(&dir).unwrap_err();
    assert!(error.contains("corpus/tier-1.json line 1"), "{error}");
    assert!(
        error.contains("carries the key \"colour\", which schema/corpus.json does not name"),
        "{error}"
    );
}

// Sabotage: with the oneOf count in CaseShape::problems ignored, this test
// fails.
#[test]
fn a_case_carrying_both_expectations_is_refused() {
    let dir = copy("both-expectations");
    edit(&dir.join("corpus/tier-1.json"), |text| {
        text.replacen(
            "{\"context\"",
            "{\"expected_error\":{\"reason\":\"x\",\"type\":\"EvaluationError\"},\"context\"",
            1,
        )
    });
    let error = Corpus::load(&dir).unwrap_err();
    assert!(
        error.contains("carries 2 of the schema's oneOf branches"),
        "{error}"
    );
}

// Sabotage: with read_case's notes read as None, this test fails.
#[test]
fn the_loader_carries_every_field_of_a_case() {
    let corpus = corpus();
    let case = corpus
        .cases
        .iter()
        .find(|case| case.id == "comparison/gt-int-boundary")
        .unwrap();
    assert_eq!(case.source.as_deref(), Some("limit > 85"));
    assert_eq!(
        case.instructions,
        decode(r#"[["load","limit"],["lit",85],["compare","GT"]]"#).unwrap()
    );
    assert_eq!(case.context, decode(r#"{"limit":85}"#).unwrap());
    assert_eq!(case.expected, Expected::Result(Value::Bool(false)));
    assert_eq!(case.tier, 1);
    assert_eq!(case.features, vec!["comparison".to_owned()]);
    assert_eq!(
        case.notes.as_deref(),
        Some("85 is not > 85 - the boundary is exclusive")
    );
    assert!(corpus.cases.iter().any(|case| matches!(
        &case.expected,
        Expected::Error { kind, reason } if !kind.is_empty() && !reason.is_empty()
    )));
}

// Sabotage: with the compiler set's source filter in Corpus::surface_set
// removed, this test fails.
#[test]
fn the_runner_answers_every_case_of_each_surface_and_skips_none() {
    let corpus = corpus();
    assert_eq!(corpus.cases.len(), 475);
    let reports = runner::observe(&corpus);
    let surfaces: Vec<&str> = reports
        .iter()
        .map(|report| report.surface.as_str())
        .collect();
    assert_eq!(surfaces, ["compiler", "evaluator"]);
    for report in &reports {
        let set = corpus.surface_set(&report.surface);
        let ids: Vec<&str> = report
            .results
            .iter()
            .map(|result| result.id.as_str())
            .collect();
        let want: Vec<&str> = set.iter().map(|case| case.id.as_str()).collect();
        assert_eq!(ids, want, "{}", report.surface);
        assert_eq!(report.corpus_hash, CORPUS_HASH);
        assert_eq!(report.tier, 10);
    }
    // The crate has no compiler yet: every compiler case fails naming it.
    assert!(reports[0].results.iter().all(|result| *result
        == CaseResult {
            id: result.id.clone(),
            outcome: Outcome::Fail("compiler not implemented".to_owned()),
        }));
    // The evaluator answers each case: a pass, or a fail saying why.
    let evaluated = |id: &str| {
        reports[1]
            .results
            .iter()
            .find(|result| result.id == id)
            .map(|result| result.outcome.clone())
    };
    assert_eq!(evaluated("comparison/gt-int-boundary"), Some(Outcome::Pass));
    assert!(matches!(
        evaluated("legacy/and-true-true"),
        Some(Outcome::Fail(_))
    ));
    let compiler = &reports[0];
    let with_source = corpus
        .cases
        .iter()
        .filter(|case| case.source.is_some())
        .count();
    assert_eq!(compiler.results.len(), with_source);
    assert!(with_source < corpus.cases.len());
    assert_eq!(reports[1].results.len(), corpus.cases.len());
}

// Sabotage: with registry::encode writing an entry's surface before its
// case_id, this test fails on the encoding check.
#[test]
fn the_vendored_registry_passes_and_claims_the_evaluator_at_tier_ten() {
    let corpus = corpus();
    let bytes = std::fs::read(root().join(registry::FILE)).unwrap();
    assert_eq!(
        registry::check(&bytes, &corpus, &runner::observe(&corpus)),
        Vec::<String>::new()
    );
    let parsed = registry::parse(&bytes).unwrap();
    assert_eq!(
        parsed.claims,
        vec![ratchet::parse_claim("evaluator:10").unwrap()]
    );
    assert_eq!(parsed.corpus_hash, CORPUS_HASH);
    assert_eq!(
        registry::unentered(&corpus, "compiler", &parsed).len(),
        corpus.surface_set("compiler").len()
    );
    assert_eq!(
        registry::unentered(&corpus, "evaluator", &parsed).len(),
        corpus.surface_set("evaluator").len() - parsed.entries.len()
    );
    // An empty array keeps its brackets on their own lines.
    assert!(registry::encode(&empty_registry(&corpus)).starts_with("{\n\"claims\": [\n],\n"));
}

// Sabotage: with the tier comparison in registry::membership removed, this
// test fails.
#[test]
fn an_entry_with_a_wrong_tier_is_refused() {
    let corpus = corpus();
    let id = "comparison/eq-string-true";
    let reports = reports_passing(&corpus, "evaluator", &[id]);
    let mut wrong = empty_registry(&corpus);
    wrong.entries.push(entry("evaluator", 2, id));
    let problems = registry::check(registry::encode(&wrong).as_bytes(), &corpus, &reports);
    assert_eq!(
        problems,
        vec![format!(
            "{id} on evaluator says tier 2; the corpus puts it in tier 1"
        )]
    );
    wrong.entries = vec![entry("evaluator", 1, id)];
    assert_eq!(
        registry::check(registry::encode(&wrong).as_bytes(), &corpus, &reports),
        Vec::<String>::new()
    );
}

// Sabotage: with registry::currency's pass lookup answering true, this test
// fails.
#[test]
fn an_entry_that_no_longer_passes_is_refused() {
    let corpus = corpus();
    let mut registry = empty_registry(&corpus);
    registry
        .entries
        .push(entry("evaluator", 1, "comparison/eq-string-true"));
    let problems = registry::check(
        registry::encode(&registry).as_bytes(),
        &corpus,
        &reports_passing(&corpus, "evaluator", &[]),
    );
    assert_eq!(
        problems,
        vec!["comparison/eq-string-true on evaluator no longer passes".to_owned()]
    );
}

// Sabotage: with registry::check's byte comparison against encode removed,
// this test fails.
#[test]
fn a_hand_edited_registry_is_refused() {
    let corpus = corpus();
    let reports = runner::observe(&corpus);
    let pretty = serde_json::to_vec_pretty(&serde_json::json!({
        "claims": [],
        "corpus_hash": CORPUS_HASH,
        "entries": [],
        "implementation": "predicator-rs",
        "isa_version": 7
    }))
    .unwrap();
    let problems = registry::check(&pretty, &corpus, &reports);
    assert_eq!(
        problems,
        vec![
            "the registry's bytes are not what re-encoding it produces; something other than the ratchet wrote it"
                .to_owned()
        ]
    );
    let extra = b"{\n\"claims\": [\n],\n\"corpus_hash\": \"x\",\n\"entries\": [\n],\n\"implementation\": \"predicator-rs\",\n\"isa_version\": 7,\n\"note\": 1\n}\n";
    let problems = registry::check(extra, &corpus, &reports);
    assert!(
        mentions(&problems, "RATCHET.md's are claims, corpus_hash"),
        "{problems:?}"
    );
}

// Sabotage: with registry::pin's corpus_hash comparison removed, this test
// fails.
#[test]
fn a_wrong_pin_is_refused() {
    let corpus = corpus();
    let mut registry = empty_registry(&corpus);
    registry.corpus_hash = format!("sha256:{}", "0".repeat(64));
    registry.isa_version = 6;
    let problems = registry::check(
        registry::encode(&registry).as_bytes(),
        &corpus,
        &runner::observe(&corpus),
    );
    assert!(
        mentions(&problems, "the registry pins sha256:000"),
        "{problems:?}"
    );
    assert!(
        mentions(&problems, "the registry's isa_version is 6"),
        "{problems:?}"
    );
}

// Sabotage: with the compiler set's source filter in Corpus::surface_set
// removed, this test fails.
#[test]
fn a_compiler_entry_for_a_case_without_a_source_is_refused() {
    let corpus = corpus();
    let case = corpus
        .cases
        .iter()
        .find(|case| case.source.is_none())
        .unwrap();
    let reports = reports_passing(&corpus, "compiler", &[case.id.as_str()]);
    let mut registry = empty_registry(&corpus);
    registry
        .entries
        .push(entry("compiler", case.tier, &case.id));
    let problems = registry::check(registry::encode(&registry).as_bytes(), &corpus, &reports);
    assert!(
        mentions(&problems, "is not in the compiler surface's case set"),
        "{problems:?}"
    );
}

// Sabotage: with registry::duplicates counting each pair once, this test
// fails.
#[test]
fn an_entry_listed_twice_and_out_of_order_is_refused() {
    let corpus = corpus();
    let id = "comparison/eq-string-true";
    let reports = reports_passing(&corpus, "evaluator", &[id, "comparison/gt-int-boundary"]);
    let line =
        |case: &str| format!("{{\"case_id\":\"{case}\",\"surface\":\"evaluator\",\"tier\":1}}");
    let bytes = format!(
        "{{\n\"claims\": [\n],\n\"corpus_hash\": \"{CORPUS_HASH}\",\n\"entries\": [\n{},\n{},\n{}\n],\n\"implementation\": \"predicator-rs\",\n\"isa_version\": 7\n}}\n",
        line("comparison/gt-int-boundary"),
        line(id),
        line(id)
    );
    let problems = registry::check(bytes.as_bytes(), &corpus, &reports);
    assert!(
        mentions(&problems, &format!("{id} on evaluator is entered twice")),
        "{problems:?}"
    );
    assert!(
        mentions(&problems, "the entries are not sorted"),
        "{problems:?}"
    );
}

// Sabotage: with the regression filter in ratchet::next removed, this test
// fails.
#[test]
fn the_ratchet_adds_observed_passes_and_refuses_a_regression() {
    let corpus = corpus();
    let id = "comparison/eq-string-true";
    let reports = reports_passing(&corpus, "evaluator", &[id]);
    let dir = copy("ratchet-adds");
    let path = dir.join(registry::FILE);
    std::fs::write(&path, registry::encode(&empty_registry(&corpus))).unwrap();
    ratchet::write(&path, &corpus, &reports, &[]).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(
        registry::parse(&bytes).unwrap().entries,
        vec![entry("evaluator", 1, id)]
    );
    assert_eq!(
        registry::check(&bytes, &corpus, &reports),
        Vec::<String>::new()
    );
    let none = reports_passing(&corpus, "evaluator", &[]);
    let refused = ratchet::write(&path, &corpus, &none, &[]).unwrap_err();
    assert_eq!(refused, vec![format!("{id} on evaluator no longer passes")]);
    assert_eq!(std::fs::read(&path).unwrap(), bytes, "nothing was written");
}

// Sabotage: with the completeness loop in ratchet::next skipped, this test
// fails.
#[test]
fn the_ratchet_writes_a_claim_only_when_the_entries_support_it() {
    let corpus = corpus();
    let tier_one: Vec<&str> = corpus
        .covered("evaluator", corpus.isa_version)
        .into_iter()
        .filter(|case| case.tier == 1)
        .map(|case| case.id.as_str())
        .collect();
    let claim = ratchet::parse_claim("evaluator:1").unwrap();
    let partial = reports_passing(&corpus, "evaluator", &tier_one[1..]);
    let refused = ratchet::next(
        &empty_registry(&corpus),
        &corpus,
        &partial,
        std::slice::from_ref(&claim),
    )
    .unwrap_err();
    assert_eq!(
        refused,
        vec![format!(
            "the claim evaluator:1 wants {}, which has no entry",
            tier_one[0]
        )]
    );
    let full = reports_passing(&corpus, "evaluator", &tier_one);
    let written = ratchet::next(
        &empty_registry(&corpus),
        &corpus,
        &full,
        std::slice::from_ref(&claim),
    )
    .unwrap();
    assert_eq!(written.claims, vec![claim]);
    assert_eq!(written.entries.len(), tier_one.len());
    assert_eq!(
        registry::check(registry::encode(&written).as_bytes(), &corpus, &full),
        Vec::<String>::new()
    );
}

// Sabotage: with ratchet::next's corpus_hash comparison removed, this test
// fails.
#[test]
fn the_ratchet_refuses_a_report_of_another_corpus() {
    let corpus = corpus();
    let mut reports = runner::observe(&corpus);
    reports[1].corpus_hash = format!("sha256:{}", "f".repeat(64));
    let refused = ratchet::next(&empty_registry(&corpus), &corpus, &reports, &[]).unwrap_err();
    assert!(
        mentions(&refused, "the evaluator report was run against sha256:fff"),
        "{refused:?}"
    );
}

// Sabotage: with parse_claim accepting any surface, this test fails.
#[test]
fn a_claim_is_read_only_as_surface_and_tier() {
    assert_eq!(
        ratchet::parse_claim("compiler:3"),
        Some(Claim {
            surface: "compiler".to_owned(),
            tier: 3
        })
    );
    for text in [
        "analysis:1",
        "evaluator:0",
        "evaluator:01",
        "evaluator",
        "evaluator:+2",
        "evaluator:x",
    ] {
        assert_eq!(ratchet::parse_claim(text), None, "{text}");
    }
}

fn superseded_with(dir: &Path, rows: &str, tag: &str) {
    std::fs::write(
        dir.join("transcript/SUPERSEDED.json"),
        format!("{{\n\"rows\": [\n{rows}\n],\n\"tag\": \"{tag}\"\n}}\n"),
    )
    .unwrap();
}

// Sabotage: with the source comparison in checks::superseded removed, this
// test fails.
#[test]
fn a_superseded_row_must_name_a_copied_row_by_index_and_source() {
    let dir = copy("superseded");
    let row = |index: u64, source: &str| {
        format!(
            "{{\"answer\":{{\"tokens\":[]}},\"file\":\"tokens.json\",\"index\":{index},\"source\":{source}}}"
        )
    };
    superseded_with(&dir, &row(1, "\"age >= 18\""), "v9.6.0");
    assert_eq!(
        checks::superseded(&dir).unwrap(),
        vec![("tokens.json".to_owned(), 1)]
    );
    assert!(checks::problems(&dir).is_empty());
    assert_eq!(
        checks::superseded_summary(&dir).unwrap(),
        "superseded transcript rows: 1 (compile.json 0, tokens.json 1)"
    );

    superseded_with(&dir, &row(1, "\"limit > 85\""), "v9.6.0");
    let error = checks::superseded(&dir).unwrap_err();
    assert!(
        error.contains("transcript/tokens.json row 1 has the source"),
        "{error}"
    );

    superseded_with(&dir, &row(5000, "\"x\""), "v9.6.0");
    assert!(
        checks::superseded(&dir)
            .unwrap_err()
            .contains("has no row 5000")
    );

    superseded_with(
        &dir,
        &format!("{},\n{}", row(1, "\"age >= 18\""), row(1, "\"age >= 18\"")),
        "v9.6.0",
    );
    assert!(
        checks::superseded(&dir)
            .unwrap_err()
            .contains("row 1 twice")
    );

    superseded_with(&dir, "", "v9.5.0");
    let error = checks::superseded(&dir).unwrap_err();
    assert!(
        error.contains("SUPERSEDED.json tag is \"v9.5.0\""),
        "{error}"
    );
    assert!(mentions(&checks::problems(&dir), "SUPERSEDED.json tag is"));
}

// Sabotage: with the file-tier comparison in Corpus::load removed, this test
// fails.
#[test]
fn a_case_in_another_tier_file_is_refused() {
    let dir = copy("wrong-tier-file");
    edit(&dir.join("corpus/tier-3.json"), |text| {
        text.replacen("\"tier\":3", "\"tier\":4", 1)
    });
    let error = Corpus::load(&dir).unwrap_err();
    assert!(
        error.contains("corpus/tier-3.json line 1: case")
            && error.contains("says tier 4, in the file of tier 3"),
        "{error}"
    );
}

// Sabotage: with the seen-id check in Corpus::load removed, this test fails.
#[test]
fn a_case_id_seen_twice_is_refused() {
    let dir = copy("twice");
    edit(&dir.join("corpus/tier-1.json"), |text| {
        let first = text.lines().next().unwrap().to_owned();
        format!("{text}{first}\n")
    });
    let error = Corpus::load(&dir).unwrap_err();
    assert!(
        error.contains("case comparison/eq-string-true is in the corpus twice"),
        "{error}"
    );
}

// Sabotage: with ratchet::next looking a pass up in the whole corpus rather
// than in the surface's set, this test fails.
#[test]
fn the_ratchet_refuses_a_pass_outside_the_surface_set() {
    let corpus = corpus();
    let case = corpus
        .cases
        .iter()
        .find(|case| case.source.is_none())
        .unwrap();
    let mut reports = runner::observe(&corpus);
    reports[0].results.push(CaseResult {
        id: case.id.clone(),
        outcome: Outcome::Pass,
    });
    let refused = ratchet::next(&empty_registry(&corpus), &corpus, &reports, &[]).unwrap_err();
    assert_eq!(
        refused,
        vec![format!(
            "the compiler report passes {}, which is not in that surface's case set",
            case.id
        )]
    );
}

// Sabotage: with the tagged writer's float text writing every float in
// exponent form, this test fails.
#[test]
fn every_case_line_round_trips_through_the_tagged_codec_byte_for_byte() {
    let dir = root();
    let mut lines = 0;
    for tier in
        crate::corpus::manifest_tiers(&crate::corpus::read_json(&dir, "manifest.json").unwrap())
            .unwrap()
    {
        let text = std::fs::read_to_string(dir.join(&tier.file)).unwrap();
        for line in text.lines().filter(|line| !line.is_empty()) {
            let value = decode(line).unwrap_or_else(|error| panic!("{}: {error:?}", tier.file));
            assert_eq!(encode(&value).unwrap(), line, "{}", tier.file);
            lines += 1;
        }
    }
    assert_eq!(lines, corpus().cases.len());
}

// Sabotage: with case_value answering Null without decoding, this test
// fails.
#[test]
fn a_case_value_the_codec_refuses_fails_the_load() {
    let dir = copy("refused-case-value");
    let tier_one = dir.join("corpus/tier-1.json");
    edit(&tier_one, |text| {
        text.replacen(
            r#""context":{"limit":85}"#,
            r#""context":{"limit":{"$type":"nope"}}"#,
            1,
        )
    });
    let error = Corpus::load(&dir).unwrap_err();
    assert!(error.contains("context: invalid_tagged_value"), "{error}");
}
