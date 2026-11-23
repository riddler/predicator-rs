# predicator-rs

A Rust port of the [predicator](https://github.com/riddler/predicator-ex)
condition language. A condition is a short expression such as
`score > 10 and status in ["open", "held"]`. The crate will compile it to a
small instruction set and evaluate it on a stack machine, with the same
answers as the reference implementation on its shared conformance corpus.

This release, 0.0.0, is a placeholder that reserves the crate name. It
exports nothing yet; the first usable version is 0.1.0.
