//! `compile` and `compile_program` against the vendored compile transcript
//! (`conformance/transcript/compile.json`), the reference compiler's own
//! answers over sources no corpus case reaches:
//!
//! - a compile row by its instruction list, written by `to_json` and read
//!   back through the tagged codec, compared operand by operand (an integer
//!   is not a float);
//! - a program row by its instruction list, the same way;
//! - a refusal row by its reason, message and position, and a program
//!   refusal row by its message and position.
//!
//! A row `conformance/transcript/SUPERSEDED.json` lists is held to the
//! answer listed there. The decompile rows render a tree back to source
//! text, which this crate has no function for; they are counted, not
//! answered.

#[cfg(test)]
mod compile_transcript {
    use predicator_rs::{Error, Instructions, Position, compile, compile_program, tagged};
    use serde_json::{Value as Json, json};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("conformance/transcript")
            .join(name)
    }

    fn rows() -> Vec<Json> {
        std::fs::read_to_string(path("compile.json"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    /// The answers SUPERSEDED.json lists for compile.json, by row index.
    fn superseded() -> BTreeMap<usize, Json> {
        let listed: Json =
            serde_json::from_str(&std::fs::read_to_string(path("SUPERSEDED.json")).unwrap())
                .unwrap();
        listed["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["file"] == "compile.json")
            .map(|row| {
                (
                    usize::try_from(row["index"].as_u64().unwrap()).unwrap(),
                    row["answer"].clone(),
                )
            })
            .collect()
    }

    /// The rows of `kind`, each with its index and the answer it is held to.
    fn rows_of(kind: &str) -> Vec<(usize, Json)> {
        let superseded = superseded();
        rows()
            .into_iter()
            .enumerate()
            .filter(|(_, row)| row["kind"] == kind)
            .map(|(index, row)| (index, superseded.get(&index).cloned().unwrap_or(row)))
            .collect()
    }

    /// The rows of `kind` whose instruction list `compiler` does not reproduce.
    fn differing_lists(
        kind: &str,
        compiler: fn(&str) -> Result<Instructions, Error>,
    ) -> Vec<String> {
        rows_of(kind)
            .into_iter()
            .filter_map(|(index, row)| {
                let source = row["source"].as_str().unwrap();
                let want = tagged::decode(&row["instructions"].to_string()).unwrap();
                let got = compiler(source)
                    .and_then(|instructions| instructions.to_json())
                    .and_then(|text| tagged::decode(&text));
                match got {
                    Ok(got) if got == want => None,
                    got => Some(format!(
                        "row {index} {}\n  want {}\n  got  {got:?}",
                        row["id"], row["instructions"]
                    )),
                }
            })
            .collect()
    }

    fn place(position: Option<Position>) -> Json {
        position.map_or(
            Json::Null,
            |at| json!({"column": at.column, "line": at.line}),
        )
    }

    // Every compile row compiles to the reference's instruction list.
    // Sabotage: with the emitter writing `jump_if_true_or_pop` for `AND`, this
    // test fails on the AND rows.
    #[test]
    fn every_compile_row_compiles_to_the_reference_instructions() {
        let differing = differing_lists("compile", compile);
        assert!(differing.is_empty(), "{}", differing.join("\n"));
        assert_eq!(rows_of("compile").len(), 63);
    }

    // Every program row compiles to the reference's instruction list.
    // Sabotage: with an `if` with an else block jumping one short past its
    // then block (Close(0) for Close(1)), this test fails on the else rows.
    #[test]
    fn every_program_row_compiles_to_the_reference_instructions() {
        let differing = differing_lists("program", compile_program);
        assert!(differing.is_empty(), "{}", differing.join("\n"));
        assert_eq!(rows_of("program").len(), 35);
    }

    // Every refusal row is refused by compile, and every program refusal row
    // by compile_program, with the row's message and position (and its reason,
    // where the row names one).
    // Sabotage: with compile refusing through an EvaluationError, this test
    // fails on every refusal row.
    #[test]
    fn every_refusal_row_is_refused_with_the_reference_answer() {
        for (kind, compiler) in [
            (
                "refusal",
                compile as fn(&str) -> Result<Instructions, Error>,
            ),
            ("program_refusal", compile_program),
        ] {
            let mut differing = Vec::new();
            for (index, row) in rows_of(kind) {
                let source = row["source"].as_str().unwrap();
                let got = match compiler(source) {
                    Ok(_) => json!("compiled"),
                    Err(error) => {
                        let mut got = json!({
                            "type": error.type_name(),
                            "message": error.message(),
                            "position": place(error.position()),
                        });
                        if row.get("reason").is_some() {
                            got["reason"] = json!(error.reason());
                        }
                        got
                    }
                };
                let mut want = json!({
                    "type": "ParseError",
                    "message": row["message"],
                    "position": row["position"],
                });
                if let Some(reason) = row.get("reason") {
                    want["reason"] = reason.clone();
                }
                if got != want {
                    differing.push(format!("row {index}\n  want {want}\n  got  {got}"));
                }
            }
            assert!(differing.is_empty(), "{kind}: {}", differing.join("\n"));
        }
    }

    // The decompile rows are counted, and every other row is one of the four
    // kinds the tests above answer, as the transcript's record counts them.
    // Sabotage: with rows() reading only the first line of compile.json, this
    // test fails on the decompile count.
    #[test]
    fn the_decompile_rows_are_counted_and_every_other_row_is_answered() {
        let record: Json =
            serde_json::from_str(&std::fs::read_to_string(path("compile-SOURCE.json")).unwrap())
                .unwrap();
        let counts = &record["counts"];
        let all = rows();
        let decompile = all.iter().filter(|row| row["kind"] == "decompile").count();
        let answered: usize = ["compile", "program", "refusal", "program_refusal"]
            .iter()
            .map(|kind| rows_of(kind).len())
            .sum();
        assert_eq!(decompile, 45);
        assert_eq!(json!(decompile), counts["decompile"]);
        assert_eq!(json!(answered + decompile), counts["rows"]);
        assert_eq!(answered + decompile, all.len());
    }
}
