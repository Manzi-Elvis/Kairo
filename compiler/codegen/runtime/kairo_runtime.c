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
extern void exit(int);
extern int getchar(void);
extern size_t strlen(const char *);
extern void *fopen(const char *, const char *);
extern size_t fread(void *, size_t, size_t, void *);
extern size_t fwrite(const void *, size_t, size_t, void *);
extern int fclose(void *);
extern int remove(const char *);

#ifdef _WIN32
extern int _write(int, const void *, unsigned int);
static void write_err(const char *s, size_t n) {
    _write(2, s, (unsigned int)n);
}
#else
extern long write(int, const void *, size_t);
static void write_err(const char *s, size_t n) {
    (void)write(2, s, n);
}
#endif

typedef struct {
    i64 len;
    void *data;
} KairoArray; /* layout matches %__kairo_array = { i64, i8* } in the generated IR */

/* ---- runtime errors: message on stderr, exit status 1 ---- */

void kairo_fatal(const char *msg) {
    write_err(msg, strlen(msg));
    write_err("\n", 1);
    exit(1);
}

static void fatal_with(const char *prefix, const char *detail) {
    write_err(prefix, strlen(prefix));
    write_err(detail, strlen(detail));
    write_err("\n", 1);
    exit(1);
}

/* ---- helpers ---- */

static char *alloc_str(size_t n) {
    char *p = (char *)malloc(n + 1);
    p[n] = 0;
    return p;
}

static void copy_bytes(char *dst, const char *src, size_t n) {
    for (size_t i = 0; i < n; i++) dst[i] = src[i];
}

static int matches_at(const char *s, size_t slen, size_t pos, const char *p, size_t plen) {
    if (pos + plen > slen) return 0;
    for (size_t i = 0; i < plen; i++) {
        if (s[pos + i] != p[i]) return 0;
    }
    return 1;
}

static i64 find_from(const char *s, size_t slen, const char *p, size_t plen, size_t from) {
    for (size_t i = from; i + plen <= slen; i++) {
        if (matches_at(s, slen, i, p, plen)) return (i64)i;
    }
    return -1;
}

static int is_space(char c) {
    return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == 0x0b || c == 0x0c;
}

/* ---- input and conversion ---- */

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
            copy_bytes(nbuf, buf, len);
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
    if (!parse_i64(s, &v)) fatal_with("invalid integer: ", s);
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

/* ---- string functions ---- */

char *kairo_substring(const char *s, i64 start, i64 end) {
    i64 len = (i64)strlen(s);
    if (start < 0 || start > len || end < start || end > len) kairo_fatal("index out of bounds");
    size_t n = (size_t)(end - start);
    char *out = alloc_str(n);
    copy_bytes(out, s + start, n);
    return out;
}

i64 kairo_index_of(const char *s, const char *p) {
    return find_from(s, strlen(s), p, strlen(p), 0);
}

int kairo_contains(const char *s, const char *p) {
    return kairo_index_of(s, p) >= 0;
}

int kairo_starts_with(const char *s, const char *p) {
    return matches_at(s, strlen(s), 0, p, strlen(p));
}

int kairo_ends_with(const char *s, const char *p) {
    size_t sl = strlen(s);
    size_t pl = strlen(p);
    return pl <= sl && matches_at(s, sl, sl - pl, p, pl);
}

char *kairo_trim(const char *s) {
    size_t a = 0;
    size_t b = strlen(s);
    while (a < b && is_space(s[a])) a++;
    while (b > a && is_space(s[b - 1])) b--;
    char *out = alloc_str(b - a);
    copy_bytes(out, s + a, b - a);
    return out;
}

char *kairo_to_upper(const char *s) {
    size_t n = strlen(s);
    char *out = alloc_str(n);
    for (size_t i = 0; i < n; i++) {
        out[i] = (s[i] >= 'a' && s[i] <= 'z') ? (char)(s[i] - 32) : s[i];
    }
    return out;
}

char *kairo_to_lower(const char *s) {
    size_t n = strlen(s);
    char *out = alloc_str(n);
    for (size_t i = 0; i < n; i++) {
        out[i] = (s[i] >= 'A' && s[i] <= 'Z') ? (char)(s[i] + 32) : s[i];
    }
    return out;
}

char *kairo_replace(const char *s, const char *from, const char *to) {
    size_t sl = strlen(s), fl = strlen(from), tl = strlen(to);
    if (fl == 0) kairo_fatal("replace: empty search string");
    size_t count = 0;
    for (size_t i = 0; i + fl <= sl;) {
        if (matches_at(s, sl, i, from, fl)) {
            count++;
            i += fl;
        } else {
            i++;
        }
    }
    char *out = alloc_str(sl - count * fl + count * tl);
    size_t o = 0;
    for (size_t i = 0; i < sl;) {
        if (i + fl <= sl && matches_at(s, sl, i, from, fl)) {
            copy_bytes(out + o, to, tl);
            o += tl;
            i += fl;
        } else {
            out[o++] = s[i++];
        }
    }
    return out;
}

KairoArray *kairo_split(const char *s, const char *sep) {
    size_t sl = strlen(s), pl = strlen(sep);
    if (pl == 0) kairo_fatal("split: empty separator");
    size_t count = 1;
    for (size_t i = 0; i + pl <= sl;) {
        if (matches_at(s, sl, i, sep, pl)) {
            count++;
            i += pl;
        } else {
            i++;
        }
    }
    char **parts = (char **)malloc(count * sizeof(char *));
    size_t k = 0, start = 0;
    for (size_t i = 0; i + pl <= sl;) {
        if (matches_at(s, sl, i, sep, pl)) {
            char *piece = alloc_str(i - start);
            copy_bytes(piece, s + start, i - start);
            parts[k++] = piece;
            i += pl;
            start = i;
        } else {
            i++;
        }
    }
    char *last = alloc_str(sl - start);
    copy_bytes(last, s + start, sl - start);
    parts[k++] = last;

    KairoArray *arr = (KairoArray *)malloc(sizeof(KairoArray));
    arr->len = (i64)k;
    arr->data = parts;
    return arr;
}

char *kairo_join(const KairoArray *arr, const char *sep) {
    size_t sl = strlen(sep);
    char **parts = (char **)arr->data;
    size_t total = 0;
    for (i64 i = 0; i < arr->len; i++) total += strlen(parts[i]);
    if (arr->len > 1) total += sl * (size_t)(arr->len - 1);
    char *out = alloc_str(total);
    size_t o = 0;
    for (i64 i = 0; i < arr->len; i++) {
        if (i > 0) {
            copy_bytes(out + o, sep, sl);
            o += sl;
        }
        size_t pl = strlen(parts[i]);
        copy_bytes(out + o, parts[i], pl);
        o += pl;
    }
    return out;
}

/* ---- files ---- */

char *kairo_read_file(const char *path) {
    void *f = fopen(path, "rb");
    if (!f) fatal_with("cannot read file: ", path);
    size_t cap = 4096;
    size_t len = 0;
    char *buf = (char *)malloc(cap + 1);
    for (;;) {
        size_t n = fread(buf + len, 1, cap - len, f);
        len += n;
        if (len < cap) break; /* short read: end of file */
        size_t ncap = cap * 2;
        char *nbuf = (char *)malloc(ncap + 1);
        copy_bytes(nbuf, buf, len);
        buf = nbuf;
        cap = ncap;
    }
    fclose(f);
    for (size_t i = 0; i < len; i++) {
        if (buf[i] == 0) fatal_with("file contains a NUL byte: ", path);
    }
    buf[len] = 0;
    return buf;
}

static void write_file(const char *path, const char *content, const char *mode) {
    void *f = fopen(path, mode);
    if (!f) fatal_with("cannot write file: ", path);
    size_t n = strlen(content);
    size_t written = fwrite(content, 1, n, f);
    fclose(f);
    if (written != n) fatal_with("cannot write file: ", path);
}

void kairo_write_file(const char *path, const char *content) {
    write_file(path, content, "wb");
}

void kairo_append_file(const char *path, const char *content) {
    write_file(path, content, "ab");
}

int kairo_file_exists(const char *path) {
    void *f = fopen(path, "rb");
    if (!f) return 0;
    fclose(f);
    return 1;
}

void kairo_delete_file(const char *path) {
    if (remove(path) != 0) fatal_with("cannot delete file: ", path);
}

/* ---- command-line arguments and exit ---- */

static int g_argc;
static char **g_argv;

void kairo_set_args(int argc, char **argv) {
    g_argc = argc;
    g_argv = argv;
}

KairoArray *kairo_args(void) {
    i64 count = g_argc > 1 ? (i64)(g_argc - 1) : 0;
    char **parts = (char **)malloc((size_t)(count > 0 ? count : 1) * sizeof(char *));
    for (i64 i = 0; i < count; i++) parts[i] = g_argv[i + 1];
    KairoArray *arr = (KairoArray *)malloc(sizeof(KairoArray));
    arr->len = count;
    arr->data = parts;
    return arr;
}

void kairo_exit(i64 code) {
    exit((int)code);
}