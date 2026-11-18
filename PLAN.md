# Plan

The single roadmap for Meng. It follows the seven milestones of the v1 backend release
plan: build, test, and deploy a REST API backed by PostgreSQL entirely in Meng.
The design decisions behind it are in [docs/design/v1-decisions.md](docs/design/v1-decisions.md)
(D1 to D8); the guides are in [docs/](docs/README.md).

Legend: ✅ done, ◐ partial (what is missing is stated), ☐ todo.

Every feature must work in **both** backends (C codegen and interpreter) with identical
output, be covered by tests (`cargo test`), and be documented in `docs/guide/`.

**History:** Phases 0 to 5 (tests, real expressions, statements, arrays and heap strings,
modules, diagnostics, dependencies, editor support, prebuilt binaries) are done; their
results are the "done" items below.

## 1. Settle language and runtime decisions

- ✅ v1 scope, platforms, compatibility policy ([D1](docs/design/v1-decisions.md#d1-v1-scope-platforms-and-compatibility))
- ✅ Concurrency model ([D2](docs/design/v1-decisions.md#d2-concurrency-model))
- ✅ Ownership of heap data ([D3](docs/design/v1-decisions.md#d3-ownership-of-data)); implementing it is in progress, see milestone 2
- ✅ Request lifetimes and caches ([D4](docs/design/v1-decisions.md#d4-request-lifetimes-and-caches))
- ✅ Native libraries ([D6](docs/design/v1-decisions.md#d6-native-libraries)), errors ([D5](docs/design/v1-decisions.md#d5-errors)), parser ([D7](docs/design/v1-decisions.md#d7-parser-before-structs-maps-and-results))
- ✅ PLAN.md follows the milestones and syntax guides live in `docs/` ([D8](docs/design/v1-decisions.md#d8-documentation-and-plan))

## 2. Complete the data and memory foundations

- ◐ Heap strings, concatenation, automatic cleanup: done on normal scope exit and early `return`; error paths wait for results (D5)
- ◐ Structs, arrays, maps: arrays with bounds checks are done; structs and maps are todo
- ☐ Optional values and typed results with propagation ([D5](docs/design/v1-decisions.md#d5-errors))
- ✅ Modules and packages (`use "file.meng";`, `use "name";`)
- ◐ Iteration: `while` is done; `for`, `break`, `continue` are todo
- ◐ Integer overflow and numeric conversion: invalid conversions are runtime errors; overflow behavior and checked parsing (needs results) are todo
- ◐ Ownership rules ([D3](docs/design/v1-decisions.md#d3-ownership-of-data)): `keep`, `reuse`, borrows, and copies exist; scalars copying and `keep` parameters borrowing are todo (foundations plan, tasks 2 and 3)
- ◐ Parser ([D7](docs/design/v1-decisions.md#d7-parser-before-structs-maps-and-results)): token-based expressions are done; the token-based statement parser (multi-line statements, exact columns) is todo (foundations plan, tasks 4 and 5)
- ◐ Regression tests for use-after-move, invalid borrows, double frees, and leaks: parser tests cover moves and borrows, CI runs generated C under AddressSanitizer; more leak tests are todo

## 3. Build the service runtime and standard library

- ☐ Tasks with `spawn` / `wait`, channels, scheduler, bounded queues ([D2](docs/design/v1-decisions.md#d2-concurrency-model))
- ☐ Networking, byte buffers, UTF-8, environment variables, clocks, structured logging ([D6](docs/design/v1-decisions.md#d6-native-libraries))
- ☐ Timeouts and cancellation through I/O ([D2](docs/design/v1-decisions.md#d2-concurrency-model), [D5](docs/design/v1-decisions.md#d5-errors))
- ☐ Service lifecycle: startup failures, signals, graceful shutdown, draining
- ◐ Dependency workflow: `deps.meow`, `meng add`, `meng deps`, a registry, and version tags are done; native libraries and a lock file for reproducible versions are todo ([D6](docs/design/v1-decisions.md#d6-native-libraries))
- ☐ Request lifetimes, caches sized from memory, SIEVE eviction ([D4](docs/design/v1-decisions.md#d4-request-lifetimes-and-caches))

## 4. Deliver HTTP and JSON support

- ☐ Select an HTTP implementation and its runtime integration ([D6](docs/design/v1-decisions.md#d6-native-libraries))
- ☐ Routing, request / response types, headers, parameters, middleware
- ☐ JSON parsing and serialization with validation errors ([D5](docs/design/v1-decisions.md#d5-errors))
- ☐ Application-error mapping to HTTP responses
- ☐ Size limits, connection limits, deadlines, malformed-request tests
- ☐ HTTPS deployment through a reverse proxy documented

## 5. Deliver PostgreSQL support

- ☐ PostgreSQL client with parameterized queries, typed rows, SQL NULL ([D6](docs/design/v1-decisions.md#d6-native-libraries))
- ☐ Pooling, transactions, query deadlines, cleanup after errors and cancellation
- ☐ TLS connections, credentials from the environment, secrets redacted from logs
- ☐ Migration workflow and integration tests against a real PostgreSQL

## 6. Prove the workflow with a reference service

- ☐ Multi-file CRUD REST API with PostgreSQL, validation, pagination, JSON errors
- ☐ Health / readiness endpoints, request IDs, structured logs, graceful shutdown
- ☐ Integration tests: success, invalid input, missing records, database outages, timeouts, rollback
- ☐ Concurrent-request and sustained-load tests with recorded thresholds
- ☐ Verify request data cleanup and cross-request ownership rules ([D4](docs/design/v1-decisions.md#d4-request-lifetimes-and-caches))
- ☐ Deployment example: database, migrations, environment, HTTPS proxy

## 7. Release tooling, documentation, and v1 acceptance

- ✅ Diagnostics with file / line / column, source excerpts, and ownership hints (exact columns for every statement come with D7)
- ◐ Build directory and commands: generated C goes to a temp folder and `meng emit` prints it; project build / test commands are todo
- ◐ CI: interpreter versus compiled output and AddressSanitizer are checked; standard library and reference service checks are todo
- ◐ Equivalent behavior across backends: compared on `test/main.meng`; error and cleanup paths are todo
- ◐ Installation binaries with checksums are published; documenting runtime / native-library dependencies is todo
- ◐ Documentation ([D8](docs/design/v1-decisions.md#d8-documentation-and-plan)): language, ownership, and tooling guides are done; errors, concurrency, HTTP / JSON, PostgreSQL, testing, and deployment guides follow their features
- ☐ Known limitations, dependency licenses, vulnerability-reporting process ([D1](docs/design/v1-decisions.md#d1-v1-scope-platforms-and-compatibility))
- ☐ Release candidate and clean-install validation
- ☐ Release v1.0 with release notes and the compatibility policy ([D1](docs/design/v1-decisions.md#d1-v1-scope-platforms-and-compatibility))

## Follow-up plans

Written after the foundations plan lands, because they build on the new parser:

- Plan 2: M2 data: structs, maps, optionals, `T or error` + `try`, `for` / `break` / `continue`
- Plan 3: D2 tasks: `spawn` / `wait`, channels, scheduler

## After v1

- An ownership explanation command that traces where values are created, moved, borrowed, and released
- Additional databases, WebSockets, distributed background jobs, more deployment targets
- Publishing the VS Code extension to the Marketplace (needs a publisher account)
