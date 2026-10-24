/* Kairo native runtime.
 *
 * `kairo build` compiles this file together with the generated LLVM IR.
 * It is deliberately header-free: the few libc functions it needs are
 * declared by hand, so building needs only a working clang and C runtime
 * library, not the system headers. Behavior must match the interpreter's
 * implementations in compiler/interpreter/src/stdlib.rs exactly.
 */

typedef __SIZE_TYPE__ size_t;
typedef long long i64;
typedef unsigned long long u64;

extern void *malloc(size_t);
extern int printf(const char *, ...);
extern void exit(int);
extern int getchar(void);

/* One line from stdin without its line ending; "" at end of input. */
char *kairo_input(void) {
    size_t cap = 64;
    size_t len = 0;
    char *buf = (char *)malloc(cap);
    int c;
    while ((c = getchar()) != -1 && c != '\n') {
        if (len + 1 >= cap) {
            size_t ncap = cap * 2;
            char *nbuf = (char *)malloc(ncap);
            for (size_t i = 0; i < len; i++) nbuf[i] = buf[i];
            buf = nbuf; /* the old buffer leaks, like all Kairo heap memory for now */
            cap = ncap;
        }
        buf[len++] = (char)c;
    }
    if (len > 0 && buf[len - 1] == '\r') len--;
    buf[len] = 0;
    return buf;
}

/* Optional sign, at least one digit, digits only, must fit in i64. */
static int parse_i64(const char *s, i64 *out) {
    int neg = 0;
    size_t i = 0;
    if (s[i] == '+' || s[i] == '-') {
        neg = (s[i] == '-');
        i++;
    }
    if (s[i] == 0) return 0;
    u64 limit = neg ? 9223372036854775808ULL : 9223372036854775807ULL;
    u64 acc = 0;
    for (; s[i]; i++) {
        if (s[i] < '0' || s[i] > '9') return 0;
        u64 d = (u64)(s[i] - '0');
        if (acc > (limit - d) / 10) return 0;
        acc = acc * 10 + d;
    }
    *out = neg ? (i64)(0 - acc) : (i64)acc;
    return 1;
}

i64 kairo_to_int(const char *s) {
    i64 v;
    if (!parse_i64(s, &v)) {
        printf("invalid integer: \"%s\"\n", s);
        exit(1);
    }
    return v;
}

int kairo_is_int(const char *s) {
    i64 v;
    return parse_i64(s, &v);
}

char *kairo_int_to_string(i64 v) {
    char tmp[24];
    int n = 0;
    u64 mag = v < 0 ? (0 - (u64)v) : (u64)v;
    do {
        tmp[n++] = (char)('0' + mag % 10);
        mag /= 10;
    } while (mag);
    char *out = (char *)malloc((size_t)n + 2);
    int j = 0;
    if (v < 0) out[j++] = '-';
    while (n) out[j++] = tmp[--n];
    out[j] = 0;
    return out;
}

char *kairo_bool_to_string(int b) {
    return (char *)(b ? "true" : "false");
}