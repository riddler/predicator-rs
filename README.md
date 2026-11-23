# predicator-rs

A Rust port of the [predicator](https://github.com/riddler/predicator-ex)
condition language. A condition is a short expression such as
`score > 10 and status in ["open", "held"]`. The crate will compile it to a
small instruction set and evaluate it on a stack machine, with the same
answers as the reference implementation on its shared conformance corpus.

The published 0.0.0 is a placeholder that reserves the crate name; the first
usable version is 0.1.0. What the source holds so far is the value model
(`Value` and its types), the errors, and the codec for the conformance
corpus's tagged-value encoding (`tagged`).

## Differences from the reference

Where the conformance corpus is silent, the reference at the tag the vendored
corpus names answers. These are the places where this crate's answer differs
from the reference's code, follows the reference's normative text where its
code says otherwise, or has no counterpart in the reference:

- **Integers are 64-bit.** An integer is an `i64`; the reference's integers
  have no bound (ruled by the operator, 2026-11-20). The tagged codec refuses
  an integer outside that range with the reason `integer_out_of_range`,
  rather than reading it as a float.
- **A map's keys are strings**, as a JSON object's are. The reference's code
  can also put an integer key into a map (a `store` whose path segment is an
  integer, into a map).
- **A duration is one kind of value**: the map of its eight units, `years`
  through `milliseconds`, the same whether a literal, an instruction or the
  host made it, as the reference's instruction-set text states it. A unit may
  be negative.
- **A duration's text writes a negative unit with its sign** (`-3d`). The
  reference's instruction-set text leaves out only a zero unit; its code
  leaves out a negative one too.
- **Two instants are equal when they are the same instant**, at any depth of
  a list or a map. The reference's code compares its instant structs, which
  can record a different precision for the same instant.
- **A float that is not finite is refused** where a value crosses into the
  crate, with the reason `non_finite_number`; the reference's floats cannot
  hold one.
- **Values are bounded at 100 levels.** Lists and maps nested past 100 levels
  are refused where a value crosses into the crate and by the tagged codec,
  with the reason `depth_limit_exceeded`; the reference has no bound. A
  deeper value the host builds is still compared, copied and dropped without
  recursion.
