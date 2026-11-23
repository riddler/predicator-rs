//! A value nested 100,000 levels deep, built without recursion, is compared,
//! ordered, searched, cloned, printed, refused at the boundary and dropped
//! on a thread with a small stack.
//!
//! A stack overflow aborts the whole process rather than failing a test, so
//! the work runs in a child process (this test binary, run again for one
//! test with an environment variable set), and the test asserts the child
//! succeeded.

use predicator_rs::{Map, Value, tagged};
use std::cmp::Ordering;
use std::process::Command;

const LEVELS: usize = 100_000;
const SMALL_STACK: usize = 256 * 1024;
const CHILD: &str = "PREDICATOR_DEEP_VALUES_CHILD";

/// A list in a map in a list ... `levels` deep, around `innermost`.
fn deep(levels: usize, innermost: i64) -> Value {
    let mut value = Value::Integer(innermost);
    for level in 0..levels {
        value = if level % 3 == 0 {
            Value::Map([("k", value)].into_iter().collect::<Map>())
        } else {
            Value::List(vec![Value::Null, value])
        };
    }
    value
}

fn deep_work() {
    let left = deep(LEVELS, 1);
    let right = deep(LEVELS, 1);
    let greater = deep(LEVELS, 2);
    assert!(left == right);
    assert!(left != greater);
    let lists = |innermost| {
        let mut value = Value::Integer(innermost);
        for _ in 0..LEVELS {
            value = Value::List(vec![value]);
        }
        value
    };
    assert!(lists(1).partial_cmp(&lists(1)) == Some(Ordering::Equal));
    assert!(lists(1).partial_cmp(&lists(2)) == Some(Ordering::Less));
    assert!(left.partial_cmp(&right) == Some(Ordering::Equal));
    let members = vec![greater.clone(), right.clone()];
    assert!(members.contains(&left));
    assert!(left.clone() == left);
    assert!(format!("{left:?}").len() > LEVELS);
    let refused = tagged::encode(&left).err();
    assert!(refused.is_some_and(|error| error.reason() == "depth_limit_exceeded"));
    drop(members);
    drop(left);
    drop(right);
    drop(greater);
}

// The work itself, run only in the child process the next test starts.
#[test]
fn deep_values_child() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    std::thread::Builder::new()
        .stack_size(SMALL_STACK)
        .spawn(deep_work)
        .unwrap()
        .join()
        .unwrap();
}

// Sabotage: with the body of Value's Drop emptied (so a value drops its
// members by recursion), this test fails.
#[test]
fn a_value_far_deeper_than_the_stack_is_handled_without_recursion() {
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "deep_values_child",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "the child failed: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}
