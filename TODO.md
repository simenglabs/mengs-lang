# TODO

See [PLAN.md](PLAN.md) for the existing compiler implementation plan.
The [v1 backend release plan](#v1-backend-release-plan) below defines the release target and dependency order for backend work.

## High priority
- [x] Commit to git
- [x] Arithmetic: `+ - * / mod`
- [x] String concatenation, `str()` / `int()` / `float()`
- [x] Reassign a variable (`x = 5;`)
- [x] Function return values (`fnc Add(a: int, b: int): int ->`)
- [x] `while` loops
- [ ] `for` loops, `break` / `continue`
- [x] Logical conditions: `and`, `or`, `not`
- [x] Parser unit tests (ownership, borrow, copy, errors)

## Medium priority
- [x] Report all errors at once, not just the first one
- [x] User input (`read`)
- [x] Arrays (`[int]`, `push`, `len`, indexing)
- [x] Heap-allocated strings + automatic `free` (where ownership saves memory)
- [x] More than one statement per line
- [x] Autocomplete: scope inside `if` blocks, hide moved variables (`meng symbols`)
- [x] `output.c` goes to a temp folder, not the current directory (`meng emit` prints the C)

## Low priority
- [x] `-O2` flag when compiling with GCC
- [x] Modules: `use "file.meng";` across files
- [x] Column in error messages (not just the line)
- [x] Extension: README, icon
- [ ] Publish the extension to the Marketplace (needs a publisher account + `VSCE_PAT` secret)
- [x] Prebuilt binaries in GitHub Releases (`.github/workflows/release.yml`)

## v1 backend release plan

**Goal:** build, test, and deploy a REST API backed by PostgreSQL entirely in Meng, without application authors writing C.

**Design direction:** clear data ownership, predictable error handling, and simple service deployment.
These are release goals; the unchecked items below are not available yet.

Work through the milestones in order. Existing unchecked tasks above remain tracked;
the milestones below identify the work required for the backend release.

### 1. Settle language and runtime decisions

- [ ] Define the v1 scope, supported platforms, and compatibility policy for language and standard-library APIs.
- [ ] Choose and document the concurrency model before designing HTTP and database APIs, including blocking I/O, request scheduling, cancellation, and shared state.
- [ ] Specify ownership for heap data: move, borrow, copy, mutation, and the meaning and cost of `keep` / `reuse`.
- [ ] Define request lifetimes and how data can outlive a request through caches or background work; prevent dangling borrows and unsafe sharing.
- [ ] Define how native libraries integrate with the runtime and how the interpreter and compiled backend preserve the same behavior.
- [ ] Update PLAN.md to follow these milestones and put language syntax guides in documentation rather than the README.

### 2. Complete the data and memory foundations

- [ ] Implement heap strings, concatenation, and automatic cleanup on normal scope exit, early return, and error paths.
- [ ] Add structs, arrays / lists, and maps with explicit ownership and bounds checks.
- [ ] Add optional values and a typed result / error mechanism with propagation and exhaustive handling.
- [ ] Add modules / imports so services and libraries can span multiple files.
- [ ] Complete iteration support, including `for`, `break`, and `continue`, with correct cleanup.
- [ ] Define integer overflow and numeric conversion behavior; add checked parsing for external input.
- [ ] Add regression tests for use-after-move, invalid borrows, double frees, and leaks; check generated native programs with memory-analysis tools.

### 3. Build the service runtime and standard library

- [ ] Implement the chosen concurrency model with bounded work queues and explicit rules for shared data.
- [ ] Add networking, byte buffers, UTF-8 handling, environment variables, clocks, and structured logging.
- [ ] Add timeouts and cancellation that release resources and propagate through I/O operations.
- [ ] Add service lifecycle support: startup failures, termination signals, graceful shutdown, and draining active requests.
- [ ] Establish a supported dependency workflow for reusable Meng modules and native libraries, with reproducible versions.

### 4. Deliver HTTP and JSON support

- [ ] Select a maintained HTTP implementation and define its runtime integration and supported protocol scope.
- [ ] Add routing, request / response types, headers, path / query parameters, and middleware.
- [ ] Add JSON parsing and serialization for structs and collections, with validation errors for invalid input.
- [ ] Define consistent application-error mapping to HTTP responses without exposing internal details.
- [ ] Add request-size limits, connection limits, read / write deadlines, and malformed-request tests.
- [ ] Document HTTPS deployment through a reverse proxy, including trusted proxy configuration.

### 5. Deliver PostgreSQL support

- [ ] Integrate a maintained PostgreSQL client with parameterized queries, typed row decoding, and explicit handling of SQL NULL.
- [ ] Support connection pooling, transactions, query deadlines, and cleanup after errors or cancellation.
- [ ] Support TLS connections and database credentials from environment configuration; redact secrets from logs.
- [ ] Provide a documented migration workflow and integration tests against a real PostgreSQL instance.

### 6. Prove the workflow with a reference service

- [ ] Build a multi-file CRUD REST API with PostgreSQL persistence, validation, pagination, and consistent JSON errors.
- [ ] Include health / readiness endpoints, request IDs, structured logs, and graceful shutdown.
- [ ] Add integration tests for successful requests, invalid input, missing records, database outages, timeouts, and transaction rollback.
- [ ] Run concurrent-request and sustained-load tests; record latency, throughput, and memory usage with a reproducible setup and acceptance thresholds.
- [ ] Verify request data is cleaned up and cross-request / background data obeys the documented ownership rules.
- [ ] Publish a deployment example with database setup, migrations, environment configuration, and HTTPS proxy configuration.

### 7. Release tooling, documentation, and v1 acceptance

- [ ] Improve diagnostics with file / line / column, source excerpts, and actionable ownership error messages.
- [ ] Add project build / test commands and keep generated C and binaries in a build directory.
- [ ] Add CI checks for the interpreter, C backend, standard library, and reference service on supported platforms.
- [ ] Verify equivalent observable behavior across interpreter and compiled execution, including error and cleanup paths.
- [ ] Publish installation binaries with checksums and document runtime / native-library dependencies so deployment does not require Rust or a C build toolchain.
- [ ] Write documentation for language syntax, ownership, errors, concurrency, HTTP / JSON, PostgreSQL, testing, and deployment; keep README concise.
- [ ] Document known limitations, dependency licenses, and a vulnerability-reporting / maintenance process.
- [ ] Cut a release candidate and validate a clean install through deployment of the reference service using only published instructions.
- [ ] Release v1.0 when all required milestones pass; publish release notes and the compatibility policy.

### After v1

- [ ] Add an ownership explanation command that traces where values are created, moved, borrowed, and released.
- [ ] Evaluate additional databases, WebSockets, distributed background jobs, and more deployment targets after the REST API workflow is stable.
