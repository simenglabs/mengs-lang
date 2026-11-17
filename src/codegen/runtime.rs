//! C helper functions, added to the generated program only when it uses them.

/// Division and remainder that stop the program on division by zero.
pub const DIVISION: &str = r#"#ifndef MENG_HELPER
#define MENG_HELPER static inline __attribute__((unused))
#endif
MENG_HELPER void meng_division_by_zero(void) {
    fflush(stdout);
    fprintf(stderr, "[ERROR] Runtime error: division by zero\n");
    exit(1);
}
MENG_HELPER long long meng_div_int(long long a, long long b) { if (b == 0) meng_division_by_zero(); return a / b; }
MENG_HELPER long long meng_mod_int(long long a, long long b) { if (b == 0) meng_division_by_zero(); return a % b; }
MENG_HELPER double meng_div_float(double a, double b) { if (b == 0) meng_division_by_zero(); return a / b; }

"#;

/// Heap strings.
///
/// - Every owned `str` variable holds a heap string and frees it when its block ends.
/// - A move hands the pointer over and sets the source to NULL (`free(NULL)` is a no-op).
/// - Strings created while evaluating a statement (`a + b`, `str(n)`, `read()`,
///   returned strings) are *temporaries*: they live in a pool that is released
///   after the statement. Each function only releases its own temporaries.
pub const STRINGS: &str = r#"#ifndef MENG_HELPER
#define MENG_HELPER static inline __attribute__((unused))
#endif
MENG_HELPER void meng_out_of_memory(void) {
    fflush(stdout);
    fprintf(stderr, "[ERROR] Runtime error: out of memory\n");
    exit(1);
}
MENG_HELPER void *meng_alloc(void *p) { if (!p) meng_out_of_memory(); return p; }

static char **meng_temps = NULL;
static size_t meng_temps_len = 0, meng_temps_cap = 0;

MENG_HELPER char *meng_temp(char *s) {
    if (meng_temps_len == meng_temps_cap) {
        meng_temps_cap = meng_temps_cap ? meng_temps_cap * 2 : 16;
        meng_temps = meng_alloc(realloc(meng_temps, meng_temps_cap * sizeof *meng_temps));
    }
    meng_temps[meng_temps_len++] = s;
    return s;
}
MENG_HELPER size_t meng_temps_mark(void) { return meng_temps_len; }
MENG_HELPER void meng_temps_release(size_t mark) {
    while (meng_temps_len > mark) free(meng_temps[--meng_temps_len]);
}

MENG_HELPER char *meng_str_dup(const char *s) {
    size_t n = strlen(s) + 1;
    return memcpy(meng_alloc(malloc(n)), s, n);
}
MENG_HELPER char *meng_take(char **slot) { char *s = *slot; *slot = NULL; return s; }
MENG_HELPER void meng_str_set(char **slot, char *value) { free(*slot); *slot = value; }

MENG_HELPER const char *meng_concat(const char *a, const char *b) {
    size_t la = strlen(a), lb = strlen(b);
    char *s = meng_alloc(malloc(la + lb + 1));
    memcpy(s, a, la);
    memcpy(s + la, b, lb + 1);
    return meng_temp(s);
}
MENG_HELPER const char *meng_int_to_str(long long n) {
    char buf[32];
    snprintf(buf, sizeof buf, "%lld", n);
    return meng_temp(meng_str_dup(buf));
}
MENG_HELPER const char *meng_float_to_str(double x) {
    char buf[64];
    snprintf(buf, sizeof buf, "%g", x);
    return meng_temp(meng_str_dup(buf));
}

MENG_HELPER void meng_conversion_error(const char *s, const char *type) {
    fflush(stdout);
    fprintf(stderr, "[ERROR] Runtime error: cannot convert \"%s\" to %s\n", s, type);
    exit(1);
}
/* `[+-]digits`, plus `.digits` for floats (same rule as the interpreter). */
MENG_HELPER int meng_is_number(const char *s, size_t len, int allow_fraction) {
    size_t i = 0, digits = 0, frac = 1;
    if (i < len && (s[i] == '+' || s[i] == '-')) i++;
    while (i < len && isdigit((unsigned char)s[i])) { i++; digits++; }
    if (allow_fraction && i < len && s[i] == '.') {
        for (i++, frac = 0; i < len && isdigit((unsigned char)s[i]); i++) frac++;
    }
    return digits > 0 && frac > 0 && i == len;
}
MENG_HELPER const char *meng_trim(const char *s, size_t *len) {
    while (isspace((unsigned char)*s)) s++;
    *len = strlen(s);
    while (*len > 0 && isspace((unsigned char)s[*len - 1])) (*len)--;
    return s;
}
MENG_HELPER long long meng_str_to_int(const char *s) {
    size_t len;
    const char *t = meng_trim(s, &len);
    if (!meng_is_number(t, len, 0)) meng_conversion_error(s, "int");
    errno = 0;
    long long n = strtoll(t, NULL, 10);
    if (errno == ERANGE) meng_conversion_error(s, "int");
    return n;
}
MENG_HELPER double meng_str_to_float(const char *s) {
    size_t len;
    const char *t = meng_trim(s, &len);
    if (!meng_is_number(t, len, 1)) meng_conversion_error(s, "float");
    return strtod(t, NULL);
}

/* One line from standard input, without the line ending ("" at end of input). */
MENG_HELPER const char *meng_read(const char *prompt) {
    fputs(prompt, stdout);
    fflush(stdout);
    size_t len = 0, cap = 64;
    char *s = meng_alloc(malloc(cap));
    int ch;
    while ((ch = getchar()) != EOF && ch != '\n') {
        if (len + 1 >= cap) s = meng_alloc(realloc(s, cap *= 2));
        s[len++] = (char)ch;
    }
    if (len > 0 && s[len - 1] == '\r') len--;
    s[len] = '\0';
    return meng_temp(s);
}

"#;
