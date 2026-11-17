%
  Meng language tour.
  Comments start and end with a percent sign and can span multiple lines.
%

fnc Example(title: str)->
    show(title);
    

    set language: str = "Meng";
    set greeting: str = language;
    show(greeting);

    show("Escape: \"quotes\", backslash \\, tab:\tok");
    show("First line\nSecond line");
<-;

fnc Greet(name: str, greeting: str)->
    show(greeting);
    show(name);
<-;

show("Hello from a home-made programming language!");
show("No GC, super fast, and friendly.");   % a percent sign inside a string is just text: %
show("100% ready");

Example("=== Inside a function ===");
Greet("Meng", "Hi, my name is:");

set keep num_1: int = 1;   % keep = constant %
set keep num_2: int = 2;
show(num_1);
show(num_2);

if num_1 = 1 ->
    show("num_1 = 1");
<-;
if num_1 < num_2 ->
    show("num_1 < num_2");
<-;
if num_1 > num_2 ->
    show("num_1 > num_2 (not printed)");
<-;
if num_1 <= num_2 ->
    show("num_1 <= num_2");
<-;
if num_1 >= num_2 ->
    show("num_1 >= num_2 (not printed)");
<-;
if num_1 != num_2 ->
    show("num_1 != num_2");

    if num_2 > 0 ->
        show("nested if: num_2 > 0");
    <-;
<-;

set language: str = "Meng";
if language = "Meng" ->
    show("strings can be compared too");
<-;

fnc Compare(keep a: int = 0, keep b: int = 0)->
    if a < b ->
        show("a is less than b");
    <- else if a = b ->
        show("a equals b");
    <- else ->
        show("a is greater than b");
    <-;
<-;

fnc Welcome(name: str, greeting: str = "Hello", repeat: int = 1)->
    show(greeting);
    show(name);
    if repeat > 1 ->
        show("(repeated)");
    <-;
<-;

Compare(num_1, num_2);
Compare(5, 5);
Compare(9);
Welcome("Meng");
Welcome("Meng", "Good morning");
Welcome("Meng", "Hey", 2);

fnc Profile(name: str, age: int, height: float = 1.7, active: bool = true)->
    show(name);
    show(age);
    show(height);
    if active ->
        show("status: active");
    <- else ->
        show("status: inactive");
    <-;
<-;

set keep owner: str = "nasri";
set keep age: int = 20;
set height: float = 1.85;
set active: bool = false;

Profile(owner, age);
Profile(owner, age, height, active);

if age > 18 ->
    show("age is a constant (keep), so it can be used again");
<-;

% Ownership: passing a variable to a function or another variable moves it. %
set reuse lang: str = "Meng";   % reuse = can be moved many times %
Greet(lang, "reuse, first call:");
Greet(lang, "reuse, second call:");

set city: str = "Jakarta";
set city_ref: str <== city;   % borrow: city stays usable %
Greet(city_ref, "borrowed:");
show(city);

set country: str = "Indonesia";
country ()=> country_copy;    % copy: an independent value %
Greet(country, "original, moved into the function:");
Greet(country_copy, "copy, still usable:");
