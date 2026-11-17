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

/* Each temporary remembers how to free itself (strings and arrays differ). */
typedef struct { void *ptr; void (*drop)(void *); } meng_temp_slot;
static meng_temp_slot *meng_temps = NULL;
static size_t meng_temps_len = 0, meng_temps_cap = 0;

MENG_HELPER void *meng_temp_with(void *ptr, void (*drop)(void *)) {
    if (meng_temps_len == meng_temps_cap) {
        meng_temps_cap = meng_temps_cap ? meng_temps_cap * 2 : 16;
        meng_temps = meng_alloc(realloc(meng_temps, meng_temps_cap * sizeof *meng_temps));
    }
    meng_temps[meng_temps_len].ptr = ptr;
    meng_temps[meng_temps_len++].drop = drop;
    return ptr;
}
MENG_HELPER char *meng_temp(char *s) { return meng_temp_with(s, free); }
MENG_HELPER size_t meng_temps_mark(void) { return meng_temps_len; }
MENG_HELPER void meng_temps_release(size_t mark) {
    while (meng_temps_len > mark) {
        meng_temps_len--;
        meng_temps[meng_temps_len].drop(meng_temps[meng_temps_len].ptr);
    }
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

/// Growable arrays (needs `STRINGS` for allocation and temporaries).
/// `kind`: 'i' int, 'f' float, 'b' bool, 's' str (the array owns its strings).
pub const ARRAYS: &str = r#"typedef struct { void *items; size_t len, cap; char kind; } meng_array;

MENG_HELPER size_t meng_elem_size(char kind) {
    return kind == 'i' ? sizeof(long long) : kind == 'f' ? sizeof(double) : kind == 'b' ? sizeof(bool) : sizeof(char *);
}
MENG_HELPER meng_array *meng_array_new(char kind) {
    meng_array *a = meng_alloc(calloc(1, sizeof *a));
    a->kind = kind;
    return a;
}
MENG_HELPER void meng_array_free(void *p) {
    meng_array *a = p;
    if (!a) return;
    if (a->kind == 's') for (size_t i = 0; i < a->len; i++) free(((char **)a->items)[i]);
    free(a->items);
    free(a);
}
MENG_HELPER meng_array *meng_array_copy(const meng_array *a) {
    meng_array *b = meng_array_new(a->kind);
    size_t size = meng_elem_size(a->kind);
    b->len = b->cap = a->len;
    b->items = meng_alloc(malloc(a->len ? a->len * size : 1));
    if (a->len) memcpy(b->items, a->items, a->len * size);
    if (a->kind == 's') for (size_t i = 0; i < b->len; i++) ((char **)b->items)[i] = meng_str_dup(((char **)a->items)[i]);
    return b;
}
MENG_HELPER meng_array *meng_temp_array(meng_array *a) { return meng_temp_with(a, meng_array_free); }
MENG_HELPER meng_array *meng_take_array(meng_array **slot) { meng_array *a = *slot; *slot = NULL; return a; }
MENG_HELPER void meng_array_set(meng_array **slot, meng_array *value) { meng_array_free(*slot); *slot = value; }
MENG_HELPER long long meng_array_len(const meng_array *a) { return (long long)a->len; }

MENG_HELPER void *meng_array_slot(meng_array *a) {
    size_t size = meng_elem_size(a->kind);
    if (a->len == a->cap) {
        a->cap = a->cap ? a->cap * 2 : 4;
        a->items = meng_alloc(realloc(a->items, a->cap * size));
    }
    return (char *)a->items + a->len++ * size;
}
MENG_HELPER size_t meng_index(const meng_array *a, long long i) {
    if (i < 0 || (size_t)i >= a->len) {
        fflush(stdout);
        fprintf(stderr, "[ERROR] Runtime error: index %lld is out of bounds (length %zu)\n", i, a->len);
        exit(1);
    }
    return (size_t)i;
}

#define MENG_ARRAY_OPS(name, T)                                                                          \
    MENG_HELPER meng_array *meng_push_##name(meng_array *a, T v) { *(T *)meng_array_slot(a) = v; return a; } \
    MENG_HELPER T meng_get_##name(const meng_array *a, long long i) { return ((T *)a->items)[meng_index(a, i)]; } \
    MENG_HELPER void meng_put_##name(meng_array *a, long long i, T v) { ((T *)a->items)[meng_index(a, i)] = v; }
MENG_ARRAY_OPS(int, long long)
MENG_ARRAY_OPS(float, double)
MENG_ARRAY_OPS(bool, bool)

/* Strings: the array owns them; `v` must be an owned heap string. */
MENG_HELPER meng_array *meng_push_str(meng_array *a, char *v) { *(char **)meng_array_slot(a) = v; return a; }
MENG_HELPER const char *meng_get_str(const meng_array *a, long long i) { return ((char **)a->items)[meng_index(a, i)]; }
MENG_HELPER void meng_put_str(meng_array *a, long long i, char *v) {
    char **slot = &((char **)a->items)[meng_index(a, i)];
    free(*slot);
    *slot = v;
}

MENG_HELPER void meng_print_array(const meng_array *a) {
    putchar('[');
    for (size_t i = 0; i < a->len; i++) {
        if (i) fputs(", ", stdout);
        switch (a->kind) {
            case 'i': printf("%lld", ((long long *)a->items)[i]); break;
            case 'f': printf("%g", ((double *)a->items)[i]); break;
            case 'b': fputs(((bool *)a->items)[i] ? "true" : "false", stdout); break;
            default: fputs(((char **)a->items)[i], stdout);
        }
    }
    puts("]");
}

"#;
