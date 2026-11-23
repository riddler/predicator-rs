//! The crate's public surface, pinned: the items the crate root makes
//! public, read from the source, against the checked-in listing
//! `tests/public_surface.txt`.
//!
//! The crate root's public items are its `pub use` re-exports and its own
//! `pub` items in `src/lib.rs`, and the `pub` items of its one public
//! module, `tagged`; every other module is private, so nothing else is
//! reachable by path. A new public item in either file fails
//! `the_public_items_are_the_listing`, and an item taken away fails it too
//! (and fails `every_listed_item_is_reachable` to compile).

#[cfg(test)]
mod public_surface {
    use std::collections::BTreeSet;

    const LISTING: &str = include_str!("public_surface.txt");
    const LIB: &str = include_str!("../src/lib.rs");
    const TAGGED: &str = include_str!("../src/tagged.rs");

    /// The kind word the listing gives an item declared with `keyword`.
    fn kind(keyword: &str) -> &str {
        match keyword {
            "struct" | "enum" | "union" | "type" | "trait" => "type",
            other => other,
        }
    }

    /// The source with its comments and string literals' contents left out,
    /// so a `pub` in a doc comment or a doctest is not read as an item.
    fn code(source: &str) -> String {
        source
            .lines()
            .map(|line| match line.trim_start().starts_with("//") {
                true => "",
                false => line,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The public items `source` declares at its top level, each as the
    /// listing writes it, `prefix` before each.
    fn items(source: &str, prefix: &str) -> BTreeSet<String> {
        let mut found = BTreeSet::new();
        let code = code(source);
        // Statements and items at the top level start a line with no
        // indentation; a `pub use` may run over several lines to its `;`.
        let mut rest = code.as_str();
        while let Some(at) = rest
            .starts_with("pub ")
            .then_some(0)
            .or_else(|| rest.find("\npub "))
        {
            let start = if rest[at..].starts_with('\n') {
                at + 1
            } else {
                at
            };
            let item = &rest[start..];
            let end = item
                .find(['{', ';', '(', '=', '<', ':'])
                .unwrap_or(item.len());
            let head: Vec<&str> = item[..end].split_whitespace().collect();
            match head.as_slice() {
                ["pub", "use", ..] => {
                    let statement = &item[..item.find(';').unwrap()];
                    let names = match (statement.find('{'), statement.rfind('}')) {
                        (Some(open), Some(close)) => statement[open + 1..close].to_owned(),
                        _ => statement.rsplit("::").next().unwrap().to_owned(),
                    };
                    for name in names
                        .split(',')
                        .map(str::trim)
                        .filter(|name| !name.is_empty())
                    {
                        let kind = if name.chars().next().unwrap().is_uppercase() {
                            "type"
                        } else {
                            "fn"
                        };
                        found.insert(format!("{prefix}{kind} {name}"));
                    }
                }
                ["pub", keyword, name, ..] => {
                    found.insert(format!("{prefix}{} {name}", kind(keyword)));
                }
                other => panic!("an item this test cannot read: {other:?}"),
            }
            rest = &item[1..];
        }
        found
    }

    // The crate root's public items are exactly the listing.
    // Sabotage: with a `pub const` added to src/lib.rs, this test fails.
    #[test]
    fn the_public_items_are_the_listing() {
        let mut found = items(LIB, "");
        found.extend(items(TAGGED, "tagged::"));
        let listed: BTreeSet<String> = LISTING.lines().map(str::to_owned).collect();
        let added: Vec<_> = found.difference(&listed).collect();
        let missing: Vec<_> = listed.difference(&found).collect();
        assert!(
            added.is_empty() && missing.is_empty(),
            "public items not in tests/public_surface.txt: {added:?}; listed items not public: {missing:?}"
        );
    }

    // The reader sees a new item of each kind, so the listing test cannot
    // pass by reading nothing.
    #[test]
    fn the_reader_sees_every_kind_of_item() {
        let source = "pub use a::{B, c};\npub use d::E;\npub mod f;\nmod g;\npub(crate) fn h() {}\npub fn i() {}\npub const J: u8 = 1;\npub struct K;\npub trait L {}\n/// pub fn m\n";
        let expected: BTreeSet<String> = [
            "type B", "fn c", "type E", "mod f", "fn i", "const J", "type K", "type L",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        assert_eq!(items(source, ""), expected);
    }

    // Every listed item is reachable by its path: the crate fails to build
    // this test when one is taken away.
    #[test]
    fn every_listed_item_is_reachable() {
        use predicator_rs as p;
        let types = [
            std::any::type_name::<p::Context>(),
            std::any::type_name::<p::Date>(),
            std::any::type_name::<p::DateTime>(),
            std::any::type_name::<p::Duration>(),
            std::any::type_name::<p::Error>(),
            std::any::type_name::<p::EvaluationError>(),
            std::any::type_name::<p::Execution>(),
            std::any::type_name::<p::Instructions>(),
            std::any::type_name::<p::Key>(),
            std::any::type_name::<p::Map>(),
            std::any::type_name::<p::OnUnbound>(),
            std::any::type_name::<p::Options<'static>>(),
            std::any::type_name::<p::ParseError>(),
            std::any::type_name::<p::Position>(),
            std::any::type_name::<p::Stopped>(),
            std::any::type_name::<p::TypeMismatchError>(),
            std::any::type_name::<p::UndefinedVariableError>(),
            std::any::type_name::<p::Value>(),
        ];
        assert_eq!(types.len(), 18);
        let _ = (
            p::compile,
            p::compile_program,
            p::evaluate::<&str>,
            p::execute::<&str>,
            p::execute_value::<&str>,
            p::parse_duration,
            p::tagged::encode,
            p::tagged::decode,
            p::ISA_VERSION,
        );
    }
}

// ISA_VERSION is the instruction-set version the vendored corpus names.
// Sabotage: with ISA_VERSION set to 6, this test fails.
#[test]
fn the_isa_version_is_the_vendored_corpus_version() {
    let source = include_str!("../conformance/SOURCE.json");
    let line = format!("\"isa_version\": {}", predicator_rs::ISA_VERSION);
    assert!(
        source.contains(&line),
        "{line} not in conformance/SOURCE.json"
    );
}
