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
- [x] Part 3-5: single-table storage, persistence
- [x] Part 6-7: cursor abstraction
- [x] Part 8-10: B-tree leaf nodes
- [x] Part 11-13: B-tree internal nodes, splitting
- [x] Part 14: duplicate keys, scanning

## Tutorial Complete!
My next project will be self-directed: a real DBMS, built in phases, each one
depending on the last.

### Phase 0 — types
- Newtype wrappers for `PageId`, `FrameId`, `TableId`, `Lsn` (not bare `usize`/`u32`).
  - `Lsn` as `NonZeroU64`, with `0` reserved as "no LSN yet".
  - Highest-leverage item on this list: most of cstack_db's real bugs were two
    different `usize`-shaped concepts colliding (a key passed where an index
    was expected, a page number passed where a cell index was expected). A
    newtype per concept turns that into a compile error.

### Phase 1 — storage layout
- Custom table schema with `String`, `int`, and `float`. `String` stored as a
  length-prefixed `VarInt`.
- Slotted page layout, fullness evaluated by free space instead of a fixed
  cell count. Byte-packed metadata (dirty bit, node type, etc).
- Sibling pointers on leaf nodes — bidirectional (prev *and* next) this time,
  to support reverse scans for roughly free.

### Phase 2 — BufferPoolManager
- Reads pages off disk, deserializes into a proper `PageNode` struct
  (`enum PageNode { Leaf(LeafData), Internal(InternalData) }`), serializes
  back to bytes only at the swap boundary (eviction or shutdown).
  - Byte manipulation lives in exactly one place: the serialize/deserialize
    pair. Everything above that boundary works with typed data, not raw
    `&mut [u8]` — this is what makes the offset/node-type-confusion bug class
    from cstack_db structurally impossible here.
- `RwLock`-guarded pages (`RwLock<Option<Box<PageFrame>>>` per frame).
  - `RwLock` chosen deliberately for real cross-thread concurrency, not by
    default — `RefCell` gets the same `&self`-based win far cheaper if this
    stays single-threaded.
- Pin counting via RAII guard (increment on fetch, decrement on `Drop`) — a
  bookkeeping mechanism, not the eviction policy itself.
- Clock eviction policy: reference bit per frame, set on access, cleared as
  the clock hand sweeps past without evicting. Only `pin_count == 0` frames
  are eligible.
- Dirty bit set on write-guard drop; only serialize-and-write on
  eviction/flush if dirty.
- One access path only (`fetch_page`, always faults in on a miss) — no
  separate forcing/non-forcing variants. Half of cstack_db's session-restart
  bugs were exactly a call site using the read-only accessor that returned
  `None` on a cache miss instead of loading from disk.

### Phase 3 — BTree over the BPM
- `BTree` struct holding a reference to the BPM (similar to `Table` and
  `Pager` here).
- Delete, including underflow handling (merge with / borrow from a sibling).
  Conspicuously absent from cstack_db — this is the mirror image of
  split-on-insert, and at least as fiddly as the key-promotion bookkeeping in
  `internal_node_split_and_insert` was, just in reverse.
- Free-list / space map for page allocation, replacing cstack_db's
  `get_unused_page_num` (which only ever grows). Needed once delete exists so
  freed pages are reusable.
- A minimal catalog/system table to durably store user-defined schema
  definitions themselves, since schemas are no longer fixed at compile time.

### Phase 4 — concurrency
- `RwLock`-guarded pages (built in Phase 2) used for real.
- Latch crabbing (lock coupling) for B+Tree traversal: hold the parent's
  latch, acquire the child's, release the parent once the child is confirmed
  safe (won't split/merge). Standard technique for concurrent tree traversal
  without one thread pinning the root latch for an entire operation.

### Phase 5 — WAL / ARIES
Hardest item on the list — sequence deliberately rather than attempting full
ARIES in one pass:
1. Redo-only logging + crash recovery first (log before touching the page,
   replay on restart).
2. Undo + CLRs (compensation log records) for transaction abort, once
   redo-only recovery is solid.
3. Invariant to never violate: the log record for a change is durable
   *before* the corresponding page write hits disk. Every page carries the
   LSN of its last modifying record.
- WAL records are generated at the BTree call site (logical redo/undo info —
  "inserted key K at slot S in page P" — lives there, not in the buffer
  pool). `WriteGuard::drop` does **not** push to the WAL; it only marks the
  frame dirty. WAL-before-data is enforced at the *other* end: in the BPM's
  flush/eviction path, before writing a dirty page, force the log manager to
  durably flush up through that page's stamped LSN.
- Build a crash-test harness as a real deliverable (kill the process
  mid-transaction, restart, assert recovery produces a consistent state) —
  the only way to know ARIES is actually correct rather than "looks right".

### Phase 6 — TransactionManager, then QueryEngine
- Basic `TransactionManager`: begin/commit/abort, transaction IDs, hooked
  into WAL. A single global lock serializing all transactions is a
  reasonable v1 concurrency model — get commit/abort/WAL integration correct
  before attempting real isolation levels (2PL/MVCC).
- Basic `QueryEngine`: thin dispatch layer once everything below it works,
  similar in spirit to `vm.rs` here.