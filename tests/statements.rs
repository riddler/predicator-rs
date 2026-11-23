//! Statement programs through the public entry points: what `execute` and
//! `execute_value` answer when a run completes and when it stops, the loop
//! budget, and the protected roots.

#[cfg(test)]
mod statements {
    use predicator_rs::{
        Context, Error, Instructions, Key, OnUnbound, Options, Value, evaluate, execute,
        execute_value,
    };

    fn program(text: &str) -> Instructions {
        Instructions::from_json(text).unwrap()
    }

    fn key(name: &str) -> Key {
        Key::from(name)
    }

    fn keys(names: &[&str]) -> Vec<Key> {
        names.iter().map(|name| key(name)).collect()
    }

    fn context(bindings: &[(&str, Value)]) -> Context {
        let mut context = Context::new();
        for (name, value) in bindings {
            context.insert(*name, value.clone());
        }
        context
    }

    /// `a = 1; b = 2; a.c = 3; d = 4`: the third statement stores through a
    /// scalar.
    const STOPS_AFTER_TWO_WRITES: &str = r#"[
        ["lit","a"],["lit",1],["store",1],
        ["lit","b"],["lit",2],["store",1],
        ["lit","a"],["lit","c"],["lit",3],["store",2],
        ["lit","d"],["lit",4],["store",1]
    ]"#;

    // Sabotage: with statements answering the context passed in on a stop
    // rather than the run's own, this test fails.
    #[test]
    fn a_run_that_stops_after_two_writes_answers_them_and_the_error() {
        let given = context(&[("z", Value::from(0))]);
        let stopped =
            execute(&program(STOPS_AFTER_TWO_WRITES), &given, &Options::new()).unwrap_err();
        assert_eq!(stopped.error().type_name(), "EvaluationError");
        assert_eq!(stopped.error().reason(), "not_a_container");
        let expected = context(&[
            ("z", Value::from(0)),
            ("a", Value::from(1)),
            ("b", Value::from(2)),
        ]);
        assert_eq!(stopped.context(), &expected);
        assert_eq!(stopped.stored_roots(), keys(&["a", "b"]));
        assert_eq!(given, context(&[("z", Value::from(0))]));
        let from_value =
            execute_value(&program(STOPS_AFTER_TWO_WRITES), &given, &Options::new()).unwrap_err();
        assert_eq!(from_value, stopped);
        assert_eq!(stopped.to_string(), stopped.error().to_string());
        assert_eq!(stopped.clone().into_context(), expected);
        let error: Error = stopped.into();
        assert_eq!(error.reason(), "not_a_container");
    }

    // Sabotage: with a Stopped converted into an Error answering a fixed
    // EvaluationError, this test fails.
    #[test]
    fn a_stopped_run_converts_into_its_error_for_the_question_mark() {
        fn run() -> Result<Context, Error> {
            Ok(execute(
                &program(STOPS_AFTER_TWO_WRITES),
                &Context::new(),
                &Options::new(),
            )?
            .into_context())
        }
        assert_eq!(run().unwrap_err().reason(), "not_a_container");
        let stopped = execute(
            &program(STOPS_AFTER_TWO_WRITES),
            &Context::new(),
            &Options::new(),
        )
        .unwrap_err();
        assert_eq!(stopped.into_error().reason(), "not_a_container");
    }

    // Sabotage: with stored roots recorded on every store rather than the
    // first, this test fails.
    #[test]
    fn a_completed_run_answers_a_new_context_and_its_roots_in_first_store_order() {
        let given = context(&[("y", Value::from(5)), ("x", Value::from(1))]);
        // y = 6; x = 1; y = 7; s = "k"
        let text = r#"[
            ["lit","y"],["lit",6],["store",1],
            ["lit","x"],["lit",1],["store",1],
            ["lit","y"],["lit",7],["store",1],
            ["lit","s"],["lit","k"],["store",1]
        ]"#;
        let execution = execute(&program(text), &given, &Options::new()).unwrap();
        assert_eq!(
            execution.context(),
            &context(&[
                ("y", Value::from(7)),
                ("x", Value::from(1)),
                ("s", Value::from("k")),
            ])
        );
        assert_eq!(execution.stored_roots(), keys(&["y", "x", "s"]));
        assert_eq!(execution.value(), None);
        assert_eq!(
            given,
            context(&[("y", Value::from(5)), ("x", Value::from(1))])
        );
        let integer_root = execute(
            &program(r#"[["lit",3],["lit",1],["store",1]]"#),
            &Context::new(),
            &Options::new(),
        )
        .unwrap();
        assert_eq!(integer_root.stored_roots(), [Key::Integer(3)]);
        assert_eq!(
            integer_root.context().get(&Key::Integer(3)),
            Some(&Value::from(1))
        );
    }

    // Sabotage: with execute_value answering undefined whatever the last pop
    // discarded, this test fails.
    #[test]
    fn execute_value_answers_what_the_last_pop_discarded() {
        let text = r#"[["lit",1],["pop"],["lit",2],["pop"],["lit",3]]"#;
        let execution = execute_value(&program(text), &Context::new(), &Options::new()).unwrap();
        assert_eq!(execution.value(), Some(&Value::from(2)));
        assert_eq!(execution.stored_roots(), [] as [Key; 0]);
        let none = execute_value(&program("[]"), &Context::new(), &Options::new()).unwrap();
        assert_eq!(none.value(), Some(&Value::Undefined));
        assert_eq!(none.into_context(), Context::new());
        // A statement program ends with an empty stack by design.
        assert!(execute(&program("[]"), &Context::new(), &Options::new()).is_ok());
        assert_eq!(
            evaluate(&program("[]"), &Context::new(), &Options::new())
                .unwrap_err()
                .reason(),
            "empty_stack"
        );
    }

    /// The type, reason and root of the error a stopped run answers.
    fn refusal(text: &str, given: &Context, options: &Options) -> (String, String, Option<String>) {
        let stopped = execute(&program(text), given, options).unwrap_err();
        let root = match stopped.error() {
            Error::Evaluation(error) => error.root().map(str::to_owned),
            _ => None,
        };
        (
            stopped.error().type_name().to_owned(),
            stopped.error().reason().to_owned(),
            root,
        )
    }

    fn protected(root: &str) -> (String, String, Option<String>) {
        (
            "EvaluationError".to_owned(),
            "protected_root".to_owned(),
            Some(root.to_owned()),
        )
    }

    // Sabotage: with store skipping the protected-root check, this test fails.
    #[test]
    fn a_protected_store_reached_through_a_loop_is_refused_at_the_attempt() {
        // i = 0; while i < 3 { i = i + 1; if i == 2 { locked = i } }
        let text = r#"[
            ["lit","i"],["lit",0],["store",1],
            ["load","i"],["lit",3],["compare","LT"],["pop_jump_if_falsy",14],
            ["lit","i"],["load","i"],["lit",1],["add"],["store",1],
            ["load","i"],["lit",2],["compare","EQ"],["pop_jump_if_falsy",4],
            ["lit","locked"],["load","i"],["store",1],
            ["jump_backward",16]
        ]"#;
        let options = Options::new().protected_roots(["locked"]);
        assert_eq!(
            refusal(text, &Context::new(), &options),
            protected("locked")
        );
        let stopped = execute(&program(text), &Context::new(), &options).unwrap_err();
        assert_eq!(stopped.context(), &context(&[("i", Value::from(2))]));
        assert_eq!(stopped.stored_roots(), keys(&["i"]));
        // Unprotected, the same program runs to its end.
        let execution = execute(&program(text), &Context::new(), &Options::new()).unwrap();
        assert_eq!(
            execution.context(),
            &context(&[("i", Value::from(3)), ("locked", Value::from(2))])
        );
        assert_eq!(execution.stored_roots(), keys(&["i", "locked"]));
    }

    // Sabotage: with the protected-root check reading the path's last segment
    // rather than its root, this test fails.
    #[test]
    fn a_protected_store_reached_through_a_jump_is_refused_per_root() {
        // if flag { x = 1 } else { locked.inner = 2 }
        let text = r#"[
            ["load","flag"],["pop_jump_if_falsy",5],
            ["lit","x"],["lit",1],["store",1],
            ["jump",5],
            ["lit","locked"],["lit","inner"],["lit",2],["store",2]
        ]"#;
        let options = Options::new().protected_roots(["locked", "other"]);
        let taken = context(&[("flag", Value::from(false))]);
        assert_eq!(refusal(text, &taken, &options), protected("locked"));
        let skipped = context(&[("flag", Value::from(true))]);
        let execution = execute(&program(text), &skipped, &options).unwrap();
        assert_eq!(execution.stored_roots(), keys(&["x"]));
        // A path whose root is not protected is written, whatever it passes
        // through.
        let through = execute(
            &program(r#"[["lit","free"],["lit","locked"],["lit",1],["store",2]]"#),
            &Context::new(),
            &options,
        )
        .unwrap();
        assert_eq!(through.stored_roots(), keys(&["free"]));
    }

    // Sabotage: with the protected-root check made before the segments are
    // checked, this test fails.
    #[test]
    fn a_protected_root_is_checked_after_the_segments_and_only_by_the_statement_entry_points() {
        let options = Options::new().protected_roots(["locked"]);
        assert_eq!(
            refusal(
                r#"[["lit","locked"],["lit",1.5],["lit",1],["store",2]]"#,
                &Context::new(),
                &options
            ),
            ("TypeMismatchError".to_owned(), "store".to_owned(), None)
        );
        let text = r#"[["lit","locked"],["lit",1],["store",1],["load","locked"]]"#;
        assert_eq!(
            evaluate(&program(text), &Context::new(), &options).unwrap(),
            Value::from(1)
        );
        // An integer root is never a protected name.
        assert!(
            execute(
                &program(r#"[["lit",1],["lit",1],["store",1]]"#),
                &Context::new(),
                &Options::new().protected_roots(["1"])
            )
            .is_ok()
        );
        let execute_value_refusal =
            execute_value(&program(text), &Context::new(), &options).unwrap_err();
        assert_eq!(execute_value_refusal.error().reason(), "protected_root");
    }

    const FOREVER: &str = r#"[["lit",true],["pop_jump_if_falsy",2],["jump_backward",2]]"#;

    /// A program that takes `n` back edges and answers `n`: a counted loop.
    fn counted(n: i64) -> String {
        format!(
            r#"[["lit","i"],["lit",0],["store",1],["load","i"],["lit",{n}],["compare","LT"],["pop_jump_if_falsy",7],["lit","i"],["load","i"],["lit",1],["add"],["store",1],["jump_backward",9],["load","i"]]"#
        )
    }

    // Sabotage: with the budget spent before the check (a budget of n letting
    // n - 1 back edges through), this test fails.
    #[test]
    fn a_run_takes_as_many_back_edges_as_its_budget_and_fails_at_the_next() {
        let at = |n: i64, budget: u64| {
            evaluate(
                &program(&counted(n)),
                &Context::new(),
                &Options::new().loop_budget(budget),
            )
        };
        assert_eq!(at(3, 3).unwrap(), Value::from(3));
        assert_eq!(at(3, 2).unwrap_err().reason(), "loop_budget_exceeded");
        assert_eq!(at(0, 0).unwrap(), Value::from(0));
        assert_eq!(at(1, 0).unwrap_err().reason(), "loop_budget_exceeded");
        assert_eq!(
            evaluate(&program(&counted(10_000)), &Context::new(), &Options::new()).unwrap(),
            Value::from(10_000)
        );
        assert_eq!(
            evaluate(&program(&counted(10_001)), &Context::new(), &Options::new())
                .unwrap_err()
                .reason(),
            "loop_budget_exceeded"
        );
        let stopped = execute(&program(FOREVER), &Context::new(), &Options::new()).unwrap_err();
        assert_eq!(stopped.error().type_name(), "EvaluationError");
        assert_eq!(stopped.error().reason(), "loop_budget_exceeded");
    }

    // Sabotage: with statements answering a refused undefined operand as the
    // TypeMismatchError itself, this test fails.
    #[test]
    fn a_statement_run_reports_an_unbound_variable_as_evaluate_does() {
        let stopped = execute(
            &program(r#"[["lit","x"],["load","missing"],["lit",1],["add"],["store",1]]"#),
            &Context::new(),
            &Options::new(),
        )
        .unwrap_err();
        assert_eq!(stopped.error().type_name(), "UndefinedVariableError");
        // An undefined value stored is no error: the answer is the context.
        let execution = execute(
            &program(r#"[["lit","x"],["load","missing"],["store",1]]"#),
            &Context::new(),
            &Options::new(),
        )
        .unwrap();
        assert_eq!(execution.context(), &context(&[("x", Value::Undefined)]));
        let strict = Options::new().on_unbound(OnUnbound::Error);
        let refused = execute(
            &program(r#"[["lit","x"],["load","missing"],["store",1]]"#),
            &Context::new(),
            &strict,
        )
        .unwrap_err();
        assert_eq!(refused.error().reason(), "unbound_variable");
        assert_eq!(refused.stored_roots(), [] as [Key; 0]);
    }

    // Sabotage: with statements skipping context.check, this test fails.
    #[test]
    fn a_context_outside_the_value_domain_stops_the_run_before_it_starts() {
        let given = context(&[("x", Value::Float(f64::NAN))]);
        let stopped = execute(
            &program(r#"[["lit","y"],["lit",1],["store",1]]"#),
            &given,
            &Options::new(),
        )
        .unwrap_err();
        assert_eq!(stopped.error().reason(), "non_finite_number");
        assert_eq!(stopped.stored_roots(), [] as [Key; 0]);
        assert_eq!(stopped.context().get(&key("y")), None);
    }
}
