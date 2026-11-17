# TODO

See [PLAN.md](PLAN.md) for the order and approach.

## High priority
- [x] Commit to git
- [x] Arithmetic: `+ - * / mod`
- [ ] String concatenation (needs heap strings)
- [x] Reassign a variable (`x = 5;`)
- [x] Function return values (`fnc Add(a: int, b: int): int ->`)
- [x] `while` loops
- [ ] `for` loops, `break` / `continue`
- [x] Logical conditions: `and`, `or`, `not`
- [x] Parser unit tests (ownership, borrow, copy, errors)

## Medium priority
- [ ] Report all errors at once, not just the first one
- [ ] User input (`read`)
- [ ] Arrays / lists
- [ ] Heap-allocated strings + automatic `free` (where ownership saves memory)
- [ ] More than one statement per line / blocks spanning lines more freely
- [ ] Autocomplete: scope inside `if` blocks, hide moved variables
- [ ] `output.c` goes to a temp folder, not the current directory

## Low priority
- [ ] `-O2` flag when compiling with GCC
- [ ] Modules / `import` across files
- [ ] Column in error messages (not just the line)
- [ ] Extension: README, icon, publish to the Marketplace
