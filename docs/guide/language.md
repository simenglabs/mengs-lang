# Language guide

This guide describes the syntax implemented today. Every `meng` example in it is
checked by `cargo test` (`tests/docs.rs`). For the rules about who owns a value, see
the [ownership guide](ownership.md); for the commands, see the [tooling guide](tooling.md).

## Programs and statements

A program is a list of statements. Every statement ends with `;`. Statements may
appear at the top level, so a small program needs no `main` function. Several
statements can share one line.

```meng
show("Hello");
show("one"); show("two");
```

## Comments

A comment starts and ends with a percent sign. It may span several lines. A percent
sign inside a string is just text.

```meng
% A comment on one line. %
%
  A comment that spans
  several lines.
%
show("100% ready");   % a comment after a statement %
```

## Types and variables

The scalar types are `str`, `int`, `float`, and `bool`. An array of a type is written
`[int]`, `[float]`, `[str]`, or `[bool]`. Every variable is declared with `set`, a name,
a type, and a value.

```meng
set name: str = "Meng";
set age: int = 20;
set height: float = 1.85;
set active: bool = true;
set scores: [int] = [70, 85];
show(name);
show(age);
show(height);
show(active);
show(scores);
```

`set` can be followed by `keep` or `reuse`:

- `set keep x: int = 1;` is a constant: it cannot be reassigned.
- `set reuse x: str = "a";` can be moved many times (it is copied when moved).

What these mean exactly is explained in the [ownership guide](ownership.md).

## Reassigning

`x = value;` gives an existing variable a new value of the same type. It is not allowed
on `keep` variables or borrows.

```meng
set count: int = 3;
count = count - 1;
show(count);
```

## Expressions

Operators, from the loosest to the tightest binding:

| Operators | Meaning |
|---|---|
| `or` | logical or |
| `and` | logical and |
| `not` | logical not |
| `=` `!=` `<` `>` `<=` `>=` | comparison (also works on strings) |
| `+` `-` | add, subtract (`+` also joins strings) |
| `*` `/` `mod` | multiply, divide, remainder |
| `-x` | unary minus |

Parentheses group. The remainder operator is spelled `mod` because `%` starts a
comment. Dividing two `int` values truncates the result (`7 / 2` is `3`); if either side
is a `float`, the result is a `float` (`7 / 2.0` is `3.5`).

```meng
set keep price: int = 7;
set keep qty: int = 3;
set total: int = price * qty + 1;
show(total);
show(price / 2);
show(price / 2.0);
show(price mod 2);
show(-(price - 10) * 2);
set big: bool = total > 20 and not (qty = 0);
show(big);
```

## Conditions

A condition is an expression of type `bool`. A block opens with `->` and closes with
`<-;`. Chain alternatives with `<- else if ... ->` and `<- else ->`.

```meng
set n: int = 5;
if n > 10 ->
    show("big");
<- else if n > 3 ->
    show("medium");
<- else ->
    show("small");
<-;
```

## Loops

`while` repeats a block as long as its condition is true.

```meng
set countdown: int = 3;
while countdown > 0 ->
    show(countdown);
    countdown = countdown - 1;
<-;
show("liftoff");
```

## Functions

A function is declared with `fnc`. Parameters have types, and a parameter may have a
default value; an argument can then be left out at the end of a call. A return type
goes after the parameter list. A function with a return type must `return` a value on
every path. Functions can be called before or after their declaration.

```meng
fnc Square(n: int): int ->
    return n * n;
<-;

fnc Describe(score: int): str ->
    if score >= 80 ->
        return "great";
    <-;
    return "keep practicing";
<-;

fnc Welcome(name: str, greeting: str = "Hello", repeat: int = 1)->
    show(greeting);
    show(name);
<-;

show(Square(4) + Square(3));
show(Describe(85));
Welcome("Meng");
Welcome("Meng", "Good morning");
```

Parameters can be marked `keep` or `reuse`, see the [ownership guide](ownership.md).

## Strings

Join strings with `+`. Convert with `str()`, `int()`, and `float()`. `len()` gives the
length. Escapes: `\"`, `\\`, `\n`, `\t`. `read(prompt)` reads a line from standard input.

```meng
set level: int = 7;
set badge: str = "[" + "Meng" + " lv." + str(level) + "]";
show(badge);
show("Quote: \"hi\", tab:\tend\nnext line");
show(int("40") + 2);
show(float("2.5") * 2);
show(len(badge));
set typed: str = read("Name: ");
show("Hello, " + typed);
```

## Arrays

Arrays hold values of one type. Index with `[i]` (the first item is `0`), change an
item by assigning to it, add with `push(array, value)`, and count with `len(array)`.
An empty `[]` takes its type from the variable it is stored in.

```meng
set reuse scores: [int] = [70, 85];
push(scores, 92);
scores[0] = 75;
show(scores);
show(scores[1]);
show(len(scores));

set pets: [str] = [];
push(pets, "cat");
show(pets);
```

## Modules and packages

`use "file.meng";` makes the functions of another file available; the path is relative
to the file that contains the `use`. `use "name";` (no `.meng`) imports an installed
package, see the [tooling guide](tooling.md). These are shown as `text` because the
files must exist.

```text
use "greetings.meng";
use "postgres";

show(Hello("imports"));
```

## Runtime errors

These stop the program with `[ERROR] Runtime error: ...` in both the interpreter and a
compiled program:

- division by zero (`/` or `mod`): `division by zero`
- an index outside the array: `index 5 is out of bounds (length 1)`
- a failed conversion: `cannot convert "zz" to int`

```text
set xs: [int] = [1];
show(xs[5]);
% [ERROR] Runtime error: index 5 is out of bounds (length 1) %
```
