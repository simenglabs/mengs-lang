# v1 design decisions

Milestone 1 of the [v1 backend release plan](../../TODO.md#v1-backend-release-plan):
settle the language and runtime decisions before building HTTP and PostgreSQL support.

Each decision lists where Meng is today, the options, a recommendation, and what it
implies. **Status: Proposed** until each one is accepted (or changed) by the maintainer.

| # | Decision | Recommendation | Status |
|---|---|---|---|
| D1 | [v1 scope, platforms, compatibility](#d1-v1-scope-platforms-and-compatibility) | Linux servers + macOS for development; semver from 1.0 | Proposed |
| D2 | [Concurrency model](#d2-concurrency-model) | Worker pool with blocking I/O; no shared mutable data | Proposed |
| D3 | [Ownership of data](#d3-ownership-of-data) | Scalars copy; heap values move; `keep` parameters borrow | Proposed |
| D4 | [Request lifetimes](#d4-request-lifetimes-and-data-that-outlives-a-request) | Request data dies with the handler; only moves leave it; borrows stay local | Proposed |
| D5 | [Errors](#d5-errors) | Typed results with explicit propagation; runtime errors fail one request | Proposed |
| D6 | [Native libraries](#d6-native-libraries) | `extern` functions + opaque handles, same C library in both backends | Proposed |
| D7 | [Parser](#d7-parser-before-structs-maps-and-results) | Token-based statement parser before structs / maps / results | Proposed |
| D8 | [Documentation](#d8-documentation-and-plan) | Syntax guides in `docs/`, short README, PLAN.md follows the milestones | Proposed |

---

## D1. v1 scope, platforms, and compatibility

**Today:** the compiler runs on macOS and Linux; release builds exist for Linux
(x86_64, arm64, static musl) and macOS (arm64, x86_64). No compatibility promise.

**Recommendation**
- **v1 scope** = what the reference service needs (TODO milestone 6): language core,
  structs / maps / results, HTTP + JSON, PostgreSQL, logging, configuration from
  the environment. Nothing else is required for 1.0.
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

| Option | Pros | Cons |
|---|---|---|
| **A. Worker pool, blocking I/O** | Simple code (no `async`), fits ownership (a request belongs to one thread), libpq is blocking anyway | One thread per in-flight request; very high connection counts need a reverse proxy in front |
| B. Event loop / async functions | Many connections per thread | `async` everywhere, callbacks or state machines in both backends, much harder cleanup and cancellation |
| C. Process per request (prefork) | Total isolation, trivial runtime | No in-process caches or pools, high memory use |
| D. Green threads (Go-like) | Best of A and B | A scheduler in the C runtime *and* the Rust interpreter: too large for v1 |

**Recommendation: A.**
- A fixed pool of N worker threads (configurable, default = CPU cores) takes requests
  from a **bounded** queue; when the queue is full, new requests get `503`.
- A request is handled entirely by one worker, from parsing to response.
- Workers share **no mutable data**. What they may share:
  - `keep` (immutable) values created at startup, such as configuration;
  - runtime-provided, internally synchronized objects: the database pool, a cache,
    a background-job queue (see D4).
- No user-visible threads, mutexes, or `async` in v1.
- **Timeouts and cancellation:** every request has a deadline. Blocking operations
  (socket reads / writes, database queries) are given the remaining time; when it runs
  out, the operation fails with a timeout error (D5) and normal cleanup runs.

**Consequences**
- The C runtime's temporaries (`meng_temps`) and any other global state must become
  per-thread (`_Thread_local`).
- The interpreter runs one interpreter instance per worker thread (its values are
  already plain data, so this is straightforward).

---

## D3. Ownership of data

**Today:** *every* variable, including `int`, is moved when assigned to another
variable or passed to a function. `keep` = constant, `reuse` = never moved,
`<==` = borrow, `()=>` = copy. Heap strings and arrays are freed automatically.
Passing a `keep` / `reuse` heap value to a function silently copies it.

**Recommendation**
1. **Scalars copy.** `int`, `float`, `bool` (and later small enums) are copied on
   assignment and when passed. Only heap values (`str`, arrays, structs, maps, handles)
   move. Moving a counter into a function today makes ordinary code awkward
   (`Repeat("ab", i)` consumes `i`), and copying them costs nothing.
2. **Heap values move** by default: on `set y: T = x;`, when passed to a function, when
   returned, pushed into an array, or stored in a struct / map.
3. **`keep` parameters borrow.** `fnc Print(keep s: str)` receives a read-only view:
   the caller keeps ownership and nothing is copied. A `keep` parameter cannot be
   moved, stored, or returned (copy it with `()=>` if it must outlive the call). This
   is the common case for functions that only read their input.
4. **Borrows (`<==`) are local and read-only.** A borrow becomes invalid when its owner is
   moved, reassigned, or changed (as today). Borrows cannot be stored in arrays,
   structs, or maps, returned, or sent elsewhere (D4). This removes dangling
   references without lifetime annotations.
5. **Copies are explicit**, with one exception: `reuse` variables are copied whenever
   they would be moved. The documentation must state that this is a full (O(n))
   copy for heap values, so `reuse` is meant for small values and convenience.
6. **`keep` variables** are immutable; because nothing can change them, the compiler
   may share them instead of copying (no visible difference), and they can be shared
   across workers (D2).
7. **Mutation:** only the owner changes a value; changing it invalidates its borrows.

**Consequences**
- Breaking change for existing code: `reuse` on scalars becomes meaningless (warning),
  and tests that expect integer moves change.
- Codegen: `keep` heap parameters become `const` pointers (no copy, no free in the
  callee).

---

## D4. Request lifetimes and data that outlives a request

**Recommendation**
- A request handler is an ordinary function. Everything it creates is owned by it and
  freed when it returns, on every path (normal return, early return, errors), using
  the existing block cleanup. No garbage collector, no request arena needed.
- Data can leave a request only by **moving** it into something the runtime owns:
  - the **response** (body, headers);
  - a **cache** (`put` moves the value in; `get` returns a copy, or a shared
    read-only view for `keep` entries);
  - the **background-job queue** (the job's arguments are moved in; the job runs later
    on a worker with its own ownership).
- Because borrows are local (D3.4), no borrow can outlive the data it points to, and
  nothing a request still uses can be freed by another thread.

**Open question:** cache eviction and size limits (per cache, in entries or bytes).

---

## D5. Errors

**Today:** compile errors are reported with file / line / column; runtime errors
(division by zero, index out of bounds, invalid conversion) print a message and exit.

**Recommendation**
- **Expected failures are values:** functions that can fail return a typed result
  (e.g. `fnc Load(id: int): User or error ->`). Callers must handle it, either
  explicitly (`if ... is error ->`) or by propagating it with a short keyword (`try`).
  Unhandled results are a compile error. Propagation runs the same cleanup as `return`.
- `error` carries a kind (not found, invalid input, timeout, database, ...) and a message;
  the HTTP layer maps kinds to status codes, and internal details are never sent to clients
  (TODO milestone 4).
- **Runtime errors** (bugs: bounds, division by zero) in a service fail **only the current
  request** with `500` and a log entry; the worker continues. Owned memory of the failed
  request is released (the runtime unwinds to the request boundary and runs the pending
  cleanups). In a plain program (no server), they keep stopping the program as today.
- SQL `NULL` and absent values use an optional type (`int?`), not a magic value.

**Consequence:** the C backend needs an unwinding mechanism (`setjmp` / `longjmp` per
request plus a cleanup list); the interpreter already returns errors as values.

---

## D6. Native libraries

**Today:** Meng cannot call C. The interpreter and compiled programs behave identically
because both are implemented in this repository.

| Option | Pros | Cons |
|---|---|---|
| **A. `extern` functions with a fixed type mapping** | One binding (in Meng) works in both backends, calling the same C library | The interpreter needs dynamic loading and a C-call mechanism (libffi) |
| B. Bindings written twice (C for compiled, Rust for interpreter) | No FFI machinery | Every native package is implemented twice; behavior can drift |
| C. Native packages only in compiled programs | Simplest | Breaks interpreter / compiled equivalence (TODO milestone 1, item 5) |

**Recommendation: A.**
```meng
extern "pq" fnc PQconnectdb(conninfo: str): handle;
extern "pq" fnc PQfinish(conn: handle);
```
- Allowed types: `int` (int64), `float` (double), `bool`, `str` (passed as a read-only C
  string; a returned C string is copied into Meng and freed with a declared function),
  and `handle` (an opaque pointer).
- A `handle` must declare its release function; the owner releases it automatically
  when its block ends, like any other heap value (so connections, results, and
  sockets are always closed).
- A package declares the system libraries it needs in its `deps.meow`; the compiler
  links them, and the interpreter loads the same shared libraries at run time.
- `extern` is allowed only in packages, so application code stays memory-safe.
- PostgreSQL then ships as an ordinary package (`meng add postgres`) that wraps libpq.

**Consequences**
- The compiler takes its first Rust dependencies (dynamic loading + libffi) for the
  interpreter.
- Deployment needs the shared library on the server (e.g. `libpq5`), documented per
  package.

---

## D7. Parser before structs, maps, and results

**Today:** statements are split by `;` and `->`; a statement must fit on one line.

**Recommendation:** replace the statement level with a token-based parser (the
expression parser already uses tokens) before adding structs, maps, results, and
JSON-heavy code, so that statements, struct literals, and argument lists can span lines.
Error columns become exact (they are found by search today). Existing syntax stays
the same.

---

## D8. Documentation and plan

**Recommendation**
- `README.md` stays short: what Meng is, install, one example, links.
- `docs/` holds the guides: language syntax, ownership, errors, concurrency,
  HTTP / JSON, PostgreSQL, testing, deployment, and these design decisions.
- `PLAN.md` is replaced by the milestones of the v1 plan (with what is already done),
  so there is a single roadmap.

---

## Already done (for reference)

These TODO items are already implemented in this repository and need no decision: heap strings with
automatic cleanup on scope exit and `return`, arrays with bounds checks, modules
(`use "file.meng";`) and packages (`deps.meow`, `meng add`, `meng deps`), diagnostics
with file / line / column and source excerpts, prebuilt binaries with checksums, and
CI that compares the interpreter with compiled programs and runs AddressSanitizer.
