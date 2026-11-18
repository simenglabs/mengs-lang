# Ownership guide

Meng has no garbage collector. Every value has one owner, and the compiler checks at
compile time that a value is not used after it was handed to someone else. This guide
describes the rules implemented today. The accepted direction for v1 (more ownership
changes) is in [design/v1-decisions.md](../design/v1-decisions.md), section
D3; this guide changes when those rules land.

## Moves

A `str` or array variable is **moved** when it is used as a bare name in one of these places:

- assigned to another variable: `set y: str = x;`
- passed to a function: `Greet(x);`
- returned: `return x;`
- pushed into an array: `push(items, x);`

After a move the old variable cannot be read until it gets a new value.

```meng
fnc Greet(name: str)->
    show(name);
<-;

set a: str = "Meng";
set b: str = a;
show(b);
Greet(b);
```

Using a moved variable is an error:

```text
set a: str = "Meng";
set b: str = a;
show(a);
```

```text
[ERROR] file.meng:3:6: variable `a` was moved on line 2 and can no longer be used; declare it with `reuse`, borrow it with `<==`, or copy it with `a ()=> new_name;`
```

## Scalars are copied

Only `str` and arrays are moved. The scalar types `int`, `float`, and `bool` are
always copied, so a scalar variable stays usable after `set b: int = a;`, after being
passed to a function, and inside loops.

```meng
fnc Double(n: int): int ->
    return n * 2;
<-;

set a: int = 21;
set b: int = a;
show(Double(a));
show(a + b);
```

`reuse` has no effect on a scalar (it is copied anyway), so the compiler prints a
warning: ``Line 1: `reuse` has no effect on int values (they are always copied)``. A
warning does not stop compilation; drop the `reuse` to silence it.

## Reading never moves

A variable that is only read is not moved: `show(x)`, a condition, or anywhere inside a
larger expression such as `a + b`, `len(x)`, or `x[0]`.

```meng
set a: int = 2;
set b: int = 3;
set total: int = a + b;
show(a);
show(total);
if a < b ->
    show("a is smaller");
<-;
```

## Reassigning makes a variable usable again

Giving a moved variable a new value makes it usable again.

```meng
set word: str = "first";
set taken: str = word;
word = "second";
show(word);
show(taken);
```

## `keep` and `reuse`

- `set keep x: T = ...;` is a constant: it is immutable and cannot be reassigned. When a
  `keep` value would be moved, it is copied instead, so it stays usable.
- `set reuse x: T = ...;` can be moved any number of times: each move copies the whole
  value. For strings and arrays this costs time proportional to the size (O(n)) every
  time, so use `reuse` for small values and convenience. On a scalar it only produces a
  warning.
- A `keep` parameter of type `str` or an array **borrows** its argument: the caller's
  variable is not moved and nothing is copied. The borrow is local and read-only: inside
  the function the parameter can be read, borrowed (`<==`), copied (`()=>`), and passed on
  to another `keep` parameter, but it cannot be moved (passed to a plain parameter,
  assigned with `set y: T = x;`, or returned) or changed, because it does not belong to
  the function. To keep or return the value, copy it first.
- A `reuse` parameter can be moved many times inside the function. A caller's `reuse`
  variable is copied into the call each time.

```meng
fnc Len(keep s: str): int ->
    return len(s);
<-;

fnc Shout(keep s: str): str ->
    s ()=> copy;
    return copy;
<-;

set word: str = "meng";
show(Len(word));
show(Len(word));
show(Shout(word));
show(word);
```

A `keep` parameter cannot escape the function:

```text
fnc Take(s: str)->
<-;
fnc F(keep s: str)->
    Take(s);
<-;

[ERROR] file.meng:4:10: `s` is a keep parameter: it is only borrowed, so it cannot be moved; copy it with `s ()=> new_name;`
```

```meng
fnc Greet(name: str)->
    show(name);
<-;

set keep owner: str = "nasri";
set reuse lang: str = "Meng";
Greet(owner);
Greet(owner);
Greet(lang);
Greet(lang);
show(owner);
```
```

## Borrow: `<==`

`set r: T <== x;` makes `r` a read-only view of `x`. The owner `x` stays usable and
nothing is copied.

```meng
fnc Greet(name: str)->
    show(name);
<-;

set city: str = "Jakarta";
set city_ref: str <== city;
Greet(city_ref);
show(city);
```

A borrow becomes invalid when its owner is moved, reassigned, or changed. Using the
borrow afterwards is an error:

```text
set c: str = "a";
set r: str <== c;
c = "b";
show(r);
```

```text
[ERROR] file.meng:4:6: `r` borrows `c`, but `c` got a new value on line 3; borrow it again
```

```text
set c: str = "a";
set r: str <== c;
set d: str = c;
show(r);
```

```text
[ERROR] file.meng:4:6: `r` borrows `c`, but `c` was moved on line 3
```

## Copy: `()=>`

`x ()=> y;` copies `x` into a new, independent variable `y`. Both stay usable.

```meng
fnc Greet(name: str)->
    show(name);
<-;

set country: str = "Indonesia";
country ()=> country_copy;
Greet(country);
Greet(country_copy);
```

## Moves inside `if` and loops

A move inside an `if` branch counts afterwards: the variable may have been moved, so it
cannot be used after the `if`.

```text
set x: str = "a";
if true ->
    set y: str = x;
<-;
show(x);
```

```text
[ERROR] file.meng:5:6: variable `x` was moved on line 3 and can no longer be used; declare it with `reuse`, borrow it with `<==`, or copy it with `x ()=> new_name;`
```

A move inside a `while` loop of a variable declared outside the loop is an error,
because the next iteration would use it again. Use `reuse`, borrow it, copy it, or give
it a new value before the loop repeats.

```text
fnc Take(s: str)->
<-;
set x: str = "a";
while true ->
    Take(x);
<-;
```

```text
[ERROR] file.meng:5:10: variable `x` is moved inside a loop, so the next iteration could not use it; declare it with `reuse`, borrow it, copy it, or give it a new value before the loop repeats
```

A variable declared inside the loop body is new in every iteration, so moving it is fine.

```meng
fnc Take(s: str)->
    show(s);
<-;

set i: int = 0;
while i < 2 ->
    set label: str = "item " + str(i);
    Take(label);
    i = i + 1;
<-;
```

## Memory

Strings and arrays live on the heap. The compiler frees them automatically when the
block that owns them ends (a function, `if`, or `while` body), including on an early
`return`. A move hands the memory over without copying it, so the new owner frees it.
There is nothing to free by hand and no way to free twice.
