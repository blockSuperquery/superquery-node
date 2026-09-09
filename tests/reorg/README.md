# Reorg scenarios

Guide Milestone 11's acceptance:

> a synthetic 3-block fork produces the same state as indexing the canonical
> branch from scratch

That is the shape every test here should take — index a fork, rewind, re-index,
and compare the result against a clean run over the canonical branch alone. A
rewind that merely "looks right" is not enough; equality with the from-scratch
run is the property that matters.

The detection half is already covered by unit tests in
`crates/core/src/finality/reorg.rs`. These are for the end-to-end behaviour, and
they need the rewind execution path (task plan phase C2) before they can be
written.
