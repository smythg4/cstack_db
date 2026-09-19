# 'Let's Build a Simple Database' Blog Follow Along
[Blog here](https://cstack.github.io/db_tutorial/)

## Porting Rules
1. Phase 1: faithful, unsafe-friendly port. Same page-as-byte-buffer layout,
   same hand-computed offsets, same function boundaries as the C original.
   Goal is passing the existing test harness, not writing idiomatic Rust.
2. Phase 2 (separate branch/commits): idiomatic refactor, once Phase 1 passes
   and I understand *why* the C original made each choice.
3. Any "this could be more Rust-y" idea that occurs to me during Phase 1 goes
   in the Deferred Improvements list below, not into the code.

## Deferred Improvements (Phase 2 candidates)
- NodeType enum instead of byte-flag branching
- Cursor<'a> borrowing from Table, vs. index-based page handles
- Page pool for the pager (single-threaded version of the
  retired/ready-to-hand-out design from the queue project)

## Non-Goals (this project)
- Concurrency of any kind — single-threaded throughout
- Crash recovery / durability beyond what the tutorial itself covers
- Anything past where the tutorial's C code actually stops
Concurrency control (coarse RwLock, later crabbing if warranted) belongs
to the follow-up DBMS project, not here.

## Progress
- [x] Part 1-2: REPL + SQL compiler skeleton
- [ ] Part 3-5: single-table storage, persistence
- [ ] Part 6-7: cursor abstraction
- [ ] Part 8-10: B-tree leaf nodes
- [ ] Part 11-13: B-tree internal nodes, splitting
- [ ] Part 14+: duplicate keys, scanning