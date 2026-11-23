//! A source passed to the entry points: `evaluate` compiles it as an
//! expression, `execute` and `execute_value` as a statement program, and a
//! source that does not compile is refused before anything runs.

#[cfg(test)]
mod sources {
    use predicator_rs::{Context, Key, Options, Value, compile, evaluate, execute, execute_value};

    fn context(bindings: &[(&str, Value)]) -> Context {
        let mut context = Context::new();
        for (name, value) in bindings {
            context.insert(*name, value.clone());
        }
        context
    }

    // evaluate runs a source as the expression it compiles to.
    // Sabotage: with the sealed trait's &str resolve answering an empty
    // instruction list, this test fails on its first assertion.
    #[test]
    fn evaluate_runs_a_source_as_an_expression() {
        let given = context(&[("score", Value::from(90)), ("tier", Value::from("gold"))]);
        let source = "score > 85 AND tier in ['gold', 'silver']";
        assert_eq!(
            evaluate(source, &given, &Options::new()),
            Ok(Value::from(true))
        );
        assert_eq!(
            evaluate(&compile(source).unwrap(), &given, &Options::new()),
            Ok(Value::from(true))
        );
    }

    // A source that does not compile is a ParseError from evaluate, and a
    // statement is not an expression.
    // Sabotage: with evaluate compiling a source as a statement program, this
    // test fails on `x = 1`, which then compiles.
    #[test]
    fn evaluate_refuses_a_source_that_is_not_an_expression() {
        let refused = evaluate("score >", &Context::new(), &Options::new()).unwrap_err();
        assert_eq!(refused.type_name(), "ParseError");
        assert_eq!(
            refused.position(),
            Some(predicator_rs::Position { line: 1, column: 8 })
        );
        let statement = evaluate("x = 1", &Context::new(), &Options::new()).unwrap_err();
        assert_eq!(statement.type_name(), "ParseError");
    }

    // execute and execute_value run a source as a statement program.
    // Sabotage: with execute compiling a source as an expression, this test
    // fails on its first assertion, the source refused.
    #[test]
    fn execute_runs_a_source_as_a_statement_program() {
        let source = "x = 1; while x < 4 { x = x * 2 } y = { a: x }; x + 1";
        let execution = execute(source, &Context::new(), &Options::new());
        assert!(execution.is_ok(), "{:?}", execution.err());
        let execution = execution.unwrap();
        assert_eq!(
            execution.context().get(&Key::from("x")),
            Some(&Value::from(4))
        );
        assert_eq!(execution.stored_roots(), [Key::from("x"), Key::from("y")]);
        assert_eq!(execution.value(), None);
        let execution = execute_value(source, &Context::new(), &Options::new()).unwrap();
        assert_eq!(execution.value(), Some(&Value::from(5)));
    }

    // A program source that does not compile stops before it runs: the
    // ParseError, the context passed in, and no roots stored.
    // Sabotage: with the &str resolve answering an empty list for a refused
    // source, this test fails on its first assertion.
    #[test]
    fn execute_stops_on_a_source_that_does_not_compile() {
        let given = context(&[("x", Value::from(1))]);
        let stopped = execute("x = 2; y =", &given, &Options::new());
        assert!(stopped.is_err());
        let stopped = stopped.unwrap_err();
        assert_eq!(stopped.error().type_name(), "ParseError");
        assert_eq!(stopped.context(), &given);
        assert!(stopped.stored_roots().is_empty());
        let stopped = execute_value("if x {", &given, &Options::new()).unwrap_err();
        assert_eq!(stopped.error().type_name(), "ParseError");
    }
}
