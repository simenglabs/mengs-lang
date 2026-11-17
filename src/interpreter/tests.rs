use super::run;
use super::value::format_g;
use crate::parser::parse;

fn output(source: &str) -> String {
    let program = parse(source).expect("test source should parse");
    let mut out = Vec::new();
    run(&program, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn prints_all_types() {
    let src = "set s: str = \"Meng\";\nset i: int = -7;\nset f: float = 2;\nset b: bool = true;\n\
               show(s);\nshow(i);\nshow(f);\nshow(b);\n";
    assert_eq!(output(src), "Meng\n-7\n2\ntrue\n");
}

#[test]
fn runs_if_else_chain() {
    let src = "set keep n: int = 0;\n\
               if n < 0 ->\n show(\"neg\");\n<- else if n = 0 ->\n show(\"zero\");\n<- else ->\n show(\"pos\");\n<-;\n";
    assert_eq!(output(src), "zero\n");
}

#[test]
fn compares_strings_and_mixed_numbers() {
    let src = "set keep a: str = \"apple\";\nset keep h: float = 1.5;\n\
               if a < \"banana\" ->\n show(\"a first\");\n<-;\n\
               if h > 1 ->\n show(\"float > int\");\n<-;\n";
    assert_eq!(output(src), "a first\nfloat > int\n");
}

#[test]
fn calls_functions_with_defaults_and_conversion() {
    let src = "fnc F(name: str, h: float = 1.7, active: bool = true)->\n\
               show(name);\nshow(h);\nif active ->\nshow(\"on\");\n<- else ->\nshow(\"off\");\n<-;\n<-;\n\
               F(\"a\");\nF(\"b\", 3, false);\n";
    assert_eq!(output(src), "a\n1.7\non\nb\n3\noff\n");
}

#[test]
fn borrow_and_copy_keep_values() {
    let src = "fnc G(s: str)->\nshow(s);\n<-;\n\
               set city: str = \"Jakarta\";\nset r: str <== city;\nG(r);\n\
               city ()=> c;\nG(city);\nG(c);\n";
    assert_eq!(output(src), "Jakarta\nJakarta\nJakarta\n");
}

#[test]
fn format_g_matches_printf() {
    let cases = [
        (1.7, "1.7"),
        (1.85, "1.85"),
        (2.0, "2"),
        (0.5, "0.5"),
        (-3.25, "-3.25"),
        (100000.0, "100000"),
        (1234567.0, "1.23457e+06"),
        (0.0001, "0.0001"),
        (0.00001, "1e-05"),
        (3.14159265, "3.14159"),
        (0.0, "0"),
    ];
    for (x, expected) in cases {
        assert_eq!(format_g(x), expected, "format_g({})", x);
    }
}

#[test]
fn evaluates_arithmetic_like_c() {
    let src = "set keep a: int = 7;\nset keep n: int = -7;\n\
               show(1 + 2 * 3);\nshow(a / 2);\nshow(a mod 2);\nshow(n / 2);\nshow(n mod 2);\n\
               show(a / 2.0);\nshow(-(a + 1));\nshow(0.1 + 0.2);\nset f: float = a / 2;\nshow(f);\n";
    assert_eq!(output(src), "7\n3\n1\n-3\n-1\n3.5\n-8\n0.3\n3\n");
}

#[test]
fn evaluates_logic_and_comparisons() {
    let src = "set keep a: int = 7;\n\
               show(a > 2 and a < 10);\nshow(a = 1 or not (a = 1));\nshow(not a > 2);\nshow(1 < 1.5);\n";
    assert_eq!(output(src), "true\ntrue\nfalse\ntrue\n");
}

#[test]
fn and_or_short_circuit() {
    // The right side would divide by zero if it were evaluated.
    let src = "set keep z: int = 0;\n\
               if z != 0 and 10 / z > 1 ->\nshow(\"no\");\n<-;\n\
               if z = 0 or 10 mod z = 1 ->\nshow(\"yes\");\n<-;\n";
    assert_eq!(output(src), "yes\n");
}

#[test]
fn division_by_zero_is_a_runtime_error() {
    for src in ["set z: int = 0;\nshow(1 / z);", "set z: int = 0;\nshow(1 mod z);", "set z: float = 0;\nshow(1.5 / z);"] {
        let program = parse(src).unwrap();
        let mut out = Vec::new();
        assert_eq!(run(&program, &mut out), Err("Runtime error: division by zero".to_string()), "{src}");
    }
}

#[test]
fn format_g_handles_non_finite() {
    assert_eq!(format_g(f64::INFINITY), "inf");
    assert_eq!(format_g(f64::NEG_INFINITY), "-inf");
    assert_eq!(format_g(f64::NAN), "nan");
}

#[test]
fn runs_while_loops_and_reassign() {
    let src = "set i: int = 3;\nwhile i > 0 ->\nshow(i);\ni = i - 1;\n<-;\n\
               set f: float = 1;\nf = 2;\nshow(f);\n";
    assert_eq!(output(src), "3\n2\n1\n2\n");
}

#[test]
fn returns_values_and_recurses() {
    let src = "fnc Fact(n: int): int ->\nif n <= 1 ->\nreturn 1;\n<-;\nreturn n * Fact(n - 1);\n<-;\n\
               fnc Half(n: int): float ->\nreturn n / 2.0;\n<-;\n\
               fnc Wide(): float ->\nreturn 3;\n<-;\n\
               show(Fact(10));\nshow(Half(7));\nshow(Wide() / 2);\n";
    assert_eq!(output(src), "3628800\n3.5\n1.5\n");
}

#[test]
fn return_leaves_loops_and_functions() {
    let src = "fnc First(limit: int)->\nset i: int = 1;\nwhile true ->\n\
               if i * i > limit ->\nshow(i);\nreturn;\n<-;\ni = i + 1;\n<-;\n<-;\n\
               First(50);\nshow(\"after\");\n";
    assert_eq!(output(src), "8\nafter\n");
}

#[test]
fn defaults_are_used_in_expression_calls() {
    let src = "fnc Scale(x: float, by: float = 2): float ->\nreturn x * by;\n<-;\n\
               show(Scale(4));\nshow(Scale(4, 0.5) + 1);\n";
    assert_eq!(output(src), "8\n3\n");
}
