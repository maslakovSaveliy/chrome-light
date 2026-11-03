# ADR-0015: stylo integration — unsafe in cl-style, Python 3 at build time, pinned version

**Status:** Accepted
**Date:** 2026-09-07
**Deciders:** project owner
**Amends:** ADR-0002 (list of unsafe crates), ADR-0010 (action item 1 — WPT adapter → M2)

## Context
ADR-0003 chose stylo as the CSS engine. Reconnaissance (2026-09-07): stylo 0.20 requires `python3` in `build.rs` (Mako codegen of properties); the embedder implements `style::dom::{TNode,TElement,...}` on `Copy` handles, while `TElement::{ensure_data, clear_data, set_dirty_descendants, ...}` are `unsafe fn`, and the `ElementData` storage needs interior mutability; Blitz (the reference) holds a raw `*mut` into the arena (open issue DioxusLabs/blitz#151). The owner confirmed stylo from day one.

## Decision
1. `cl-style` is the fifth crate allowed to use `unsafe`, only in the modules `store.rs`, `handle.rs`, `stylo_dom.rs`; the handle is a `Copy` triple `(&Document, &StyleStore, NodeId)`, not a raw pointer; `ElementData` is a side table for one style pass (`Box<[UnsafeCell<Option<ElementData>>]>`), recreated on every `resolve()`; traversal is sequential only (`traverse_dom(.., None)`); `debug_assert` thread checks at every `unsafe` entry.
2. `servo_arc`/`style::*` do not leave `crates/style/**` and `crates/layout/src/style_adapt.rs` (script `tools/check-stylo-scope.sh`).
3. Python 3 ≥ 3.10 is a build dependency; CI installs `actions/setup-python@v5` in every job.
4. Versions are pinned in `[workspace.dependencies]`: stylo 0.20.0, stylo_traits 0.20.0, stylo_dom 0.20.0, stylo_atoms 0.20.0, selectors 0.40, cssparser 0.37, html5ever/markup5ever 0.39 (one markup5ever version in the tree — checked with `cargo tree -d`). Updates only as a whole, in a separate PR.
5. `cargo miri` is impossible for cl-style (build.rs/codegen); instead nightly `cargo careful test -p cl-style` + `careful` in nightly CI.
6. **Option B (fallback):** if the trait-integration task does not pass the test `p_with_color_rule_should_resolve_to_red` after 3 attempts / 5 working days — a minimal own cascade behind the same API `cl_style::{StyleEngine, StyledDocument, computed()}` (cssparser + selectors, ~20 longhand properties, no @media/custom properties); stylo returns in M2. The decision is recorded here as "Option B taken" with a date.

## Consequences
- Easier: a Firefox-grade cascade, selectors, @media, custom properties for free.
- Harder: build time, Python in CI, unsafe review for three modules, the Windows build of stylo.
- Revisit: on a stylo major version change, or if Blitz#151 yields a safe pattern — switch to it.

## Amendment 2026-09-09 (after Task 11)

Decision item 1 assumed that the per-node `ElementData` storage would have to be built as
`Box<[UnsafeCell<Option<ElementData>>]>`, because stylo calls `unsafe fn ensure_data(&self)`
through a `Copy` handle. **That turned out to be wrong and, moreover, infeasible** against stylo 0.20:
`ElementDataMut`/`ElementDataRef` have private fields and are created exclusively by
`ElementDataWrapper::{borrow, borrow_mut}` (`stylo-0.20.0/style/data.rs`), and `ElementDataWrapper`
itself already contains the needed interior mutability (plus a debug `AtomicRefCell` tracker). Our own
`UnsafeCell` here is not only redundant — there is nothing to fill it with.

Actual implementation: `StyleStore` is safe `Cell`s and one `ElementDataWrapper` per node.
**There is not a single `unsafe` block in production code.** `store.rs` and `handle.rs` kept
`#![deny(unsafe_code)]`; the `#![allow(unsafe_code)]` exception is needed only in `stylo_dom.rs`, and only
because *implementing* the five `unsafe fn` methods of the `TElement` trait counts as
`unsafe_code` by itself — no body contains `unsafe { }`. The single-thread invariant is kept as a
`debug_assert_eq!` on the owning thread at every mutating entry point; `StyleStore` is not `Sync`.

**Two clarifications without which the amendment would mislead** (found by the Task 11 review):

1. **The obligation did not vanish together with the keyword.** `ElementDataWrapper::borrow_mut(&self)`
   is a safe function that hands out `&mut` **without a runtime check in release**: the tracker field
   `refcell` is declared under `#[cfg(debug_assertions)]`. An aliasing violation is UB in release and a panic
   in debug. So `cargo careful` (item 5) and the debug build in CI are not "nice to have" — they carry
   weight. This is exactly Gecko's position (`gecko/wrapper.rs`), not a weakness of ours.
2. **"Sequential traversal only" remains a runtime rule, not a consequence of the type system.**
   `!Sync` on `StyleStore` guarantees nothing: stylo declares `unsafe impl Send` for `SendNode`
   and `SendElement` **unconditionally, without an `N: Send` bound** (`stylo-0.20.0/style/dom.rs`), and
   `driver::traverse_dom` wraps the root in `SendNode` on every call. So
   `traverse_dom(.., Some(pool))` would compile and race. The ban must be a check at
   run time (a hard `assert!`), not a hope on the compiler.

What this changes:
- Read Decision item 1 as "the storage is per-pass, access only through stylo's API; `unsafe` is allowed,
  but in fact was not needed".
- Item 5 (`cargo miri` impossible) stays in force because of stylo's `build.rs` codegen, but
  the value of miri for `cl-style` is now close to zero: there is nothing to check.
- Option B (item 6) is **not activated**: the gate `p_with_color_rule_should_resolve_to_red` is still ahead
  (Task 13), but the traits, selector matching and the storage work.

## Amendment 2026-09-10 (after Task 13): the handle must be exactly one word wide

Task 11 made the handle a `Copy` triple `(&Document, &StyleStore, NodeId)` — and the review rightly
praised it as a safer alternative to Blitz's raw pointer. **Task 13 showed that
stylo does not accept such a handle.** `StyleSharingCache` declares a mirror structure
`FakeCandidate { _element: usize, .. }` and checks in `SharingCache::new`
`size_of::<SharingCache<E>>() == size_of::<TypelessSharingCache>()`
(`stylo-0.20.0/sharing/mod.rs:324,611-613`). `ThreadLocalStyleContext::new` creates this cache on
every pass, so the three-word handle aborted **every** style pass with `left: 10000, right: 9488`.

That, not carelessness, explains the raw pointer in Blitz: stylo requires
`TElement` to be exactly pointer-width.

Our solution: `NodeHandle` is a one-word reference into a per-pass `NodeArena`, whose nodes are linked
to each other through `Cell`. **Still zero `unsafe`**; the width is pinned by a `const` assertion next to the
type. Task 11's invariants (the owner-thread check, `store.slot_count() == doc.len()`)
are kept, but now live in the arena.

What this changes in Decision item 1: read "`Copy` triple, not a raw pointer" as
"a one-word reference into an arena, not a raw pointer". The motive is the same (no `unsafe`, lifetimes
checked by the compiler), and the width constraint is an external requirement of stylo, not our choice.
