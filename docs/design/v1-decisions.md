# v1 design decisions

Milestone 1 of the [v1 backend release plan](../../TODO.md#v1-backend-release-plan):
settle the language and runtime decisions before building HTTP and PostgreSQL support.

Each decision lists where Meng is today, the options, the chosen direction, and what
it implies.

| # | Decision | Direction | Status |
|---|---|---|---|
| D1 | [v1 scope, platforms, compatibility](#d1-v1-scope-platforms-and-compatibility) | Linux servers + macOS for development; semver from 1.0 | Accepted |
| D2 | [Concurrency model](#d2-concurrency-model) | Lightweight tasks (like goroutines), many per thread; `spawn` / `wait` for async work | Accepted (revised) |
| D3 | [Ownership of data](#d3-ownership-of-data) | Scalars copy; heap values move; `keep` parameters borrow | Accepted |
| D4 | [Request lifetimes and caches](#d4-request-lifetimes-and-caches) | Request data dies with its task; caches sized from the server's memory, SIEVE eviction + TTL | Accepted (revised) |
| D5 | [Errors](#d5-errors) | Typed results with explicit propagation; runtime errors fail one task | Accepted |
| D6 | [Native libraries](#d6-native-libraries) | `extern` functions + opaque handles, same C library in both backends | Accepted |
| D7 | [Parser](#d7-parser-before-structs-maps-and-results) | Token-based statement parser before structs / maps / results | Accepted |
| D8 | [Documentation](#d8-documentation-and-plan) | Syntax guides in `docs/`, short README, PLAN.md follows the milestones | Accepted |

---

## D1. v1 scope, platforms, and compatibility

**Today:** the compiler runs on macOS and Linux; release builds exist for Linux
(x86_64, arm64, static musl) and macOS (arm64, x86_64). No compatibility promise.

**Decision**
- **v1 scope** = what the reference service needs (TODO milestone 6): language core,
  structs / maps / results, tasks, HTTP + JSON, PostgreSQL, logging, configuration
  from the environment. Nothing else is required for 1.0.
- **Platforms:** Linux x86_64 and arm64 are the *deployment* targets (fully tested,
  CI-gated). macOS arm64 and x86_64 are *development* targets (tested, but services
  are not supported in production there). Windows is out of scope for v1.
- **Compatibility:** before 1.0, anything may change (breaking changes are listed in
  release notes). From 1.0, semantic versioning: no breaking change to the language,
  the standard library, `deps.meow`, or the CLI within 1.x; anything to be removed is
  deprecated (with a compiler warning) for at least one minor release first.

**Consequence:** CI must run the reference service on Linux; macOS stays a
build-and-test target only.

---

## D2. Concurrency model

**Today:** single-threaded. The C runtime keeps its temporaries in global variables.

**Decision: lightweight tasks, many per OS thread (like Go's goroutines), with
`spawn` / `wait` for asynchronous work.**

### Tasks
- A **task** is a function running with its own small stack. Thousands of tasks can
  run on a few OS threads: the runtime starts one scheduler thread per CPU core
  (configurable) and moves tasks between them (work stealing) to keep cores busy.
- Code is written **sequentially**, without colored functions: a task that reads a
  socket or waits for a database simply calls the function; when the operation would
  block, the task is parked and the thread runs another task. When the socket or query
  is ready, the task resumes (I/O readiness comes from `epoll` on Linux and `kqueue`
  on macOS).
- Every HTTP request runs in its own task.

### Async: `spawn`, `wait`, channels
```meng
set user_task: task[User or error] = spawn LoadUser(id);   % starts now %
set orders_task: task[[Order] or error] = spawn LoadOrders(id);
set user: User or error = wait user_task;                  % async: wait for the result %
set orders: [Order] or error = wait orders_task;
```
- `spawn F(args)` starts `F` in a new task and returns a `task[T]` handle at once.
  `wait t` parks the current task until `t` finishes and gives its result. Several
  tasks run at the same time, so the two queries above overlap.
- `wait` with a deadline returns a timeout error (D5) instead of waiting forever.
- **Channels** (`chan[T]`) connect tasks: `send(c, value)` moves a value in, `receive(c)`
  takes it out. A channel has a capacity; `send` waits when it is full (backpressure).
- A `task` handle is owned: if its owner's block ends without `wait`, the task is
  cancelled (so no work runs on unnoticed).

### Ownership across tasks (no data races)
- `spawn` **moves** its arguments into the new task; borrows cannot be passed
  (D3.4). `send` moves values into a channel. `wait` / `receive` move the result out.
- Tasks share no mutable data. They may share `keep` (immutable) values and
  runtime objects that synchronize internally: the database pool, caches (D4), channels.
- So every value belongs to exactly one task at a time, and data races cannot be written.

### Limits and cancellation
- Requests are admitted up to `max_requests` (D4 computes the default); above that,
  new requests get `503`.
- Every request task has a deadline. Waiting operations (socket, database, `wait`,
  `receive`) get the remaining time and fail with a timeout error when it runs out;
  cancelling a task makes its next waiting operation fail the same way. Normal cleanup
  then runs.

### Implementation
- **C runtime:** stackful coroutines with a small context switch per architecture
  (x86_64, arm64), stacks of 64 KiB by default (configurable), allocated with a guard
  page so overflow is caught instead of corrupting memory; only touched pages use RAM.
  The scheduler, run queues, and the I/O poller are part of the runtime.
- **Interpreter:** the same scheduler model with stackful coroutines in Rust (a crate
  such as `corosensei`), so programs behave the same in both backends. Only timing and
  the interleaving of concurrent tasks may differ, as in any concurrent program.
- **Per-task state:** the temporaries pool (`meng_temps`) and other runtime state
  become *per task*, not global or per thread, because a task can move between threads.
- **Blocking C calls** (D6): an `extern` function marked `blocking` runs on a separate
  thread pool while its task is parked, so it never stalls a scheduler thread. Where a
  library has a non-blocking API (libpq does), packages should use it.

---

## D3. Ownership of data

**Today:** *every* variable, including `int`, is moved when assigned to another
variable or passed to a function. `keep` = constant, `reuse` = never moved,
`<==` = borrow, `()=>` = copy. Heap strings and arrays are freed automatically.
Passing a `keep` / `reuse` heap value to a function silently copies it.

**Decision**
1. **Scalars copy.** `int`, `float`, `bool` (and later small enums) are copied on
   assignment and when passed. Only heap values (`str`, arrays, structs, maps, handles,
   tasks, channels) move.
2. **Heap values move** by default: on `set y: T = x;`, when passed to a function or
   `spawn`, when returned, sent on a channel, pushed into an array, or stored in a
   struct / map.
3. **`keep` parameters borrow.** `fnc Print(keep s: str)` receives a read-only view:
   the caller keeps ownership and nothing is copied. A `keep` parameter cannot be
   moved, stored, returned, or passed to `spawn` (copy it with `()=>` if it must
   outlive the call).
4. **Borrows (`<==`) are local and read-only.** A borrow becomes invalid when its owner is
   moved, reassigned, or changed (as today). Borrows cannot be stored in arrays,
   structs, or maps, returned, sent, or passed to another task. This removes dangling
   references without lifetime annotations.
5. **Copies are explicit**, with one exception: `reuse` variables are copied whenever
   they would be moved. The documentation must state that this is a full (O(n))
   copy for heap values, so `reuse` is meant for small values and convenience.
6. **`keep` variables** are immutable; because nothing can change them, the compiler
   may share them instead of copying (no visible difference), and they can be shared
   between tasks (D2).
7. **Mutation:** only the owner changes a value; changing it invalidates its borrows.

**Consequences**
- Breaking change for existing code: `reuse` on scalars becomes meaningless (warning),
  and tests that expect integer moves change.
- Codegen: `keep` heap parameters become `const` pointers (no copy, no free in the
  callee).

---

## D4. Request lifetimes and caches

### Request data
- A request handler is an ordinary function running in its own task. Everything it
  creates is owned by it and freed when it returns, on every path (normal return,
  early return, errors, cancellation), using the existing block cleanup. No garbage
  collector and no request arena are needed.
- Data can leave a request only by **moving** it into something the runtime owns:
  the **response**, a **cache**, a **channel**, or a task started with `spawn`.
- Because borrows are local (D3.4), no borrow can outlive the data it points to, and
  nothing a task still uses can be freed by another task.

### Cache API
```meng
set keep sessions: cache[str, Session] = cache(max: "64MB", expire: "30m");
set keep pages: cache[str, str] = cache();          % size from the default budget %

put(sessions, token, session);      % moves `session` into the cache %
set s: Session? = get(sessions, token);   % a copy (or a shared view for `keep` values) %
remove(sessions, token);
```
- A cache is a runtime object that synchronizes itself (D2). Declaring it `keep` fixes
  the variable (it cannot be reassigned or moved) and lets every task share it; its
  contents still change through `put`, `remove`, and `clear`.
- `max` sets the cache's size in bytes (`"64MB"`, `"1GB"`); `entries:` can limit the
  number of entries as well. `expire` is an optional time to live (TTL).
- A cache's size counts the memory of its keys and values (the runtime knows the
  allocation size of strings, arrays, structs, and maps), plus a small per-entry overhead.

### Default size: computed from the server

When `max` is not given, caches share a **cache budget** computed at startup.

```
M  = memory available to the service:
       the container limit if there is one (cgroup v2 memory.max, or cgroup v1
       memory.limit_in_bytes), otherwise the machine's physical memory
       (sysconf(_SC_PHYS_PAGES) × page size)

max_requests (default) = clamp(M / 2 MiB, 64, 10 000)
reserved               = 64 MiB (runtime, code, buffers)
                       + max_requests × 256 KiB (working memory per in-flight request)
                       = 64 MiB + M / 8           (while max_requests is not clamped)

cache budget           = clamp(25% × (M − reserved), 8 MiB, 4 GiB)
                       = clamp(0.25 × (0.875 × M − 64 MiB), 8 MiB, 4 GiB)
```

| Memory available (M) | max_requests | reserved | Cache budget |
|---|---|---|---|
| 256 MiB | 128 | 96 MiB | 40 MiB |
| 512 MiB | 256 | 128 MiB | 96 MiB |
| 1 GiB | 512 | 192 MiB | 208 MiB |
| 2 GiB | 1 024 | 320 MiB | 432 MiB |
| 4 GiB | 2 048 | 576 MiB | 880 MiB |
| 8 GiB | 4 096 | 1 088 MiB | 1 776 MiB |
| 16 GiB | 8 192 | 2 112 MiB | 3 568 MiB |
| 32 GiB | 10 000 (cap) | ≈ 2 564 MiB | 4 GiB (cap) |

Why these numbers:
- **25%** leaves most memory for request work, the database client, and spikes; a cache
  is an optimization, so it should never be what pushes the service out of memory.
- **256 KiB per request** is a planning figure for an average in-flight request (body,
  JSON, query results). It only decides the defaults; it is not a hard limit per request.
- The **container limit** comes first because services usually run in containers, where
  physical memory is much larger than what the service may use.
- The **8 MiB floor** keeps caches useful on tiny machines; the **4 GiB cap** avoids huge
  defaults on large servers (set `max` explicitly to use more).

Sharing the budget:
- Caches with an explicit `max` take that amount first (they may exceed the budget;
  the program asked for it).
- Caches without `max` split what remains equally.
- Overrides: `MENG_CACHE_BUDGET` (e.g. `512MB`) replaces the computed budget, and
  `MENG_MAX_REQUESTS` replaces `max_requests`.
- At startup the runtime logs the result, e.g.
  `cache budget 432 MiB (memory limit 2 GiB from cgroup, max_requests 1024)`.

### Eviction: recommendation

| Policy | Hit ratio | Cost per read | Notes |
|---|---|---|---|
| FIFO | low | none | evicts popular entries just because they are old |
| LRU | good | moves the entry to the front of a list (a write under a lock) | the familiar default; a single scan of many keys flushes it |
| LFU | good for stable popularity | counter updates | slow to adapt when popularity changes |
| W-TinyLFU | best | sketch updates + two lists | complex to implement twice (C runtime and interpreter) |
| **SIEVE** | as good as or better than LRU on web workloads | sets one "visited" bit | simple, scan-resistant, cheap under concurrency |

**Recommendation: SIEVE, plus TTL and size awareness.**
1. **SIEVE** (NSDI 2024): entries sit in one FIFO queue with a *visited* bit. A read
   only sets the bit. To make room, a "hand" moves from the oldest entry toward newer
   ones: a visited entry gets its bit cleared and stays, the first unvisited entry is
   evicted. Popular entries survive; one-time entries (e.g. a crawler scanning many
   pages) leave quickly. Reads never reorder anything, so many tasks can read at once.
2. **TTL first:** an expired entry is never returned (checked on `get`), and expired
   entries are removed before any live entry is evicted. A background sweep removes
   expired entries a little at a time.
3. **Size aware:** evict until the new entry fits. An entry larger than 1/8 of its cache
   is not stored (`put` returns an error), so one huge value cannot empty the cache.
4. **Concurrency:** each cache is split into shards (16 by default, chosen by key hash),
   each with its own lock, queue, and hand.
5. **Explicit control:** `remove`, and `clear` for the whole cache, for invalidation
   after writes.

---

## D5. Errors

**Today:** compile errors are reported with file / line / column; runtime errors
(division by zero, index out of bounds, invalid conversion) print a message and exit.

**Decision**
- **Expected failures are values:** functions that can fail return a typed result
  (e.g. `fnc Load(id: int): User or error ->`). Callers must handle it, either
  explicitly (`if ... is error ->`) or by propagating it with a short keyword (`try`).
  Unhandled results are a compile error. Propagation runs the same cleanup as `return`.
- `error` carries a kind (not found, invalid input, timeout, cancelled, database, ...)
  and a message; the HTTP layer maps kinds to status codes, and internal details are
  never sent to clients (TODO milestone 4).
- **Runtime errors** (bugs: bounds, division by zero) fail **only the current task**:
  for a request, the client gets `500` and the error is logged; `wait` on a failed task
  returns an error. Owned memory of the failed task is released (the runtime unwinds
  the task and runs its pending cleanups). Other tasks keep running. In a plain program
  (no server), a runtime error in the main task stops the program as today.
- SQL `NULL` and absent values use an optional type (`int?`), not a magic value.

**Consequence:** the C backend unwinds a failed task through its pending cleanups
(each task already has its own stack, so unwinding stops at the task boundary);
the interpreter already returns errors as values.

---

## D6. Native libraries

**Today:** Meng cannot call C. The interpreter and compiled programs behave identically
because both are implemented in this repository.

| Option | Pros | Cons |
|---|---|---|
| **A. `extern` functions with a fixed type mapping** | One binding (in Meng) works in both backends, calling the same C library | The interpreter needs dynamic loading and a C-call mechanism (libffi) |
| B. Bindings written twice (C for compiled, Rust for interpreter) | No FFI machinery | Every native package is implemented twice; behavior can drift |
| C. Native packages only in compiled programs | Simplest | Breaks interpreter / compiled equivalence (TODO milestone 1, item 5) |

**Decision: A.**
```meng
extern "pq" fnc PQconnectStart(conninfo: str): handle;
extern "pq" blocking fnc PQconnectdb(conninfo: str): handle;   % runs off the scheduler (D2) %
extern "pq" fnc PQfinish(conn: handle);
```
- Allowed types: `int` (int64), `float` (double), `bool`, `str` (passed as a read-only C
  string; a returned C string is copied into Meng and freed with a declared function),
  and `handle` (an opaque pointer).
- A `handle` must declare its release function; the owner releases it automatically
  when its block ends, like any other heap value (so connections, results, and
  sockets are always closed).
- `blocking` marks calls that may wait (D2); the runtime runs them on a separate thread
  pool. Packages also get access to the I/O poller, so non-blocking C APIs (libpq's
  `PQsendQuery` / `PQsocket` / `PQconsumeInput`) can park the task until the socket is ready.
- A package declares the system libraries it needs in its `deps.meow`; the compiler
  links them, and the interpreter loads the same shared libraries at run time.
- `extern` is allowed only in packages, so application code stays memory-safe.
- PostgreSQL then ships as an ordinary package (`meng add postgres`) that wraps libpq.

**Consequences**
- The compiler takes its first Rust dependencies (dynamic loading + libffi, and
  coroutines for D2) for the interpreter.
- Deployment needs the shared library on the server (e.g. `libpq5`), documented per
  package.

---

## D7. Parser before structs, maps, and results

**Today:** statements are split by `;` and `->`; a statement must fit on one line.

**Decision:** replace the statement level with a token-based parser (the expression
parser already uses tokens) before adding structs, maps, results, and JSON-heavy code,
so that statements, struct literals, and argument lists can span lines. Error columns
become exact (they are found by search today). Existing syntax stays the same.

---

## D8. Documentation and plan

**Decision**
- `README.md` stays short: what Meng is, install, one example, links.
- `docs/` holds the guides: language syntax, ownership, errors, concurrency,
  HTTP / JSON, PostgreSQL, testing, deployment, and these design decisions.
- `PLAN.md` is replaced by the milestones of the v1 plan (with what is already done),
  so there is a single roadmap.

---

## Already done (for reference)

These TODO items are already implemented in this repository and need no decision:
heap strings with automatic cleanup on scope exit and `return`, arrays with bounds
checks, modules (`use "file.meng";`) and packages (`deps.meow`, `meng add`,
`meng deps`), diagnostics with file / line / column and source excerpts, prebuilt
binaries with checksums, and CI that compares the interpreter with compiled programs
and runs AddressSanitizer.
