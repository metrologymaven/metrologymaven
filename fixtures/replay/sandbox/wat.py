#!/usr/bin/env python3
"""Minimal WebAssembly text-format assembler for the TRUST.REPLAY.SANDBOX fixtures (stdlib only).

Supports the subset the sandbox fixtures use, in the flat (non-folded) instruction syntax:
module fields `type`, `import` (func, memory, global, table), `func` (inline `export`, `param`,
`result`, `local`, optional `$names`), `memory`, `table`, `global`, `export`, `start`, `elem`
(legacy active form) and `data` (active form); instructions of WebAssembly 1.0 plus sign
extension, saturating truncation and `memory.copy`/`memory.fill`. Labels may be numeric depths
or `$names`. Float constants are written as `i32.const`/`i64.const` bit patterns followed by a
reinterpretation, so every constant is exact.

This is an independent encoder written from the core specification's binary format; it shares
no code with the Rust sandbox it tests.
"""

# ------------------------------------------------------------------ opcodes

NUMERIC = (
    "i32.eqz i32.eq i32.ne i32.lt_s i32.lt_u i32.gt_s i32.gt_u i32.le_s i32.le_u i32.ge_s i32.ge_u "
    "i64.eqz i64.eq i64.ne i64.lt_s i64.lt_u i64.gt_s i64.gt_u i64.le_s i64.le_u i64.ge_s i64.ge_u "
    "f32.eq f32.ne f32.lt f32.gt f32.le f32.ge f64.eq f64.ne f64.lt f64.gt f64.le f64.ge "
    "i32.clz i32.ctz i32.popcnt i32.add i32.sub i32.mul i32.div_s i32.div_u i32.rem_s i32.rem_u "
    "i32.and i32.or i32.xor i32.shl i32.shr_s i32.shr_u i32.rotl i32.rotr "
    "i64.clz i64.ctz i64.popcnt i64.add i64.sub i64.mul i64.div_s i64.div_u i64.rem_s i64.rem_u "
    "i64.and i64.or i64.xor i64.shl i64.shr_s i64.shr_u i64.rotl i64.rotr "
    "f32.abs f32.neg f32.ceil f32.floor f32.trunc f32.nearest f32.sqrt "
    "f32.add f32.sub f32.mul f32.div f32.min f32.max f32.copysign "
    "f64.abs f64.neg f64.ceil f64.floor f64.trunc f64.nearest f64.sqrt "
    "f64.add f64.sub f64.mul f64.div f64.min f64.max f64.copysign "
    "i32.wrap_i64 i32.trunc_f32_s i32.trunc_f32_u i32.trunc_f64_s i32.trunc_f64_u "
    "i64.extend_i32_s i64.extend_i32_u i64.trunc_f32_s i64.trunc_f32_u i64.trunc_f64_s i64.trunc_f64_u "
    "f32.convert_i32_s f32.convert_i32_u f32.convert_i64_s f32.convert_i64_u f32.demote_f64 "
    "f64.convert_i32_s f64.convert_i32_u f64.convert_i64_s f64.convert_i64_u f64.promote_f32 "
    "i32.reinterpret_f32 i64.reinterpret_f64 f32.reinterpret_i32 f64.reinterpret_i64 "
    "i32.extend8_s i32.extend16_s i64.extend8_s i64.extend16_s i64.extend32_s"
).split()
assert len(NUMERIC) == 0xC4 - 0x45 + 1

LOADS = (
    "i32.load i64.load f32.load f64.load i32.load8_s i32.load8_u i32.load16_s i32.load16_u "
    "i64.load8_s i64.load8_u i64.load16_s i64.load16_u i64.load32_s i64.load32_u"
).split()
STORES = "i32.store i64.store f32.store f64.store i32.store8 i32.store16 i64.store8 i64.store16 i64.store32".split()
NATURAL = {
    "i32.load": 2, "i64.load": 3, "f32.load": 2, "f64.load": 3, "i32.load8_s": 0, "i32.load8_u": 0,
    "i32.load16_s": 1, "i32.load16_u": 1, "i64.load8_s": 0, "i64.load8_u": 0, "i64.load16_s": 1,
    "i64.load16_u": 1, "i64.load32_s": 2, "i64.load32_u": 2, "i32.store": 2, "i64.store": 3,
    "f32.store": 2, "f64.store": 3, "i32.store8": 0, "i32.store16": 1, "i64.store8": 0,
    "i64.store16": 1, "i64.store32": 2,
}
SAT = (
    "i32.trunc_sat_f32_s i32.trunc_sat_f32_u i32.trunc_sat_f64_s i32.trunc_sat_f64_u "
    "i64.trunc_sat_f32_s i64.trunc_sat_f32_u i64.trunc_sat_f64_s i64.trunc_sat_f64_u"
).split()

PLAIN = {"unreachable": 0x00, "nop": 0x01, "return": 0x0F, "drop": 0x1A, "select": 0x1B}
for _i, _n in enumerate(NUMERIC):
    PLAIN[_n] = 0x45 + _i

VALTYPES = {"i32": 0x7F, "i64": 0x7E, "f32": 0x7D, "f64": 0x7C}


class WatError(Exception):
    pass


# ------------------------------------------------------------------ encoding helpers

def uleb(n):
    if n < 0:
        raise WatError(f"negative unsigned {n}")
    out = bytearray()
    while True:
        b = n & 0x7F
        n >>= 7
        if n:
            out.append(b | 0x80)
        else:
            out.append(b)
            return bytes(out)


def sleb(n):
    out = bytearray()
    while True:
        b = n & 0x7F
        n >>= 7
        if (n == 0 and not b & 0x40) or (n == -1 and b & 0x40):
            out.append(b)
            return bytes(out)
        out.append(b | 0x80)


def vec(items):
    return uleb(len(items)) + b"".join(items)


def name(s):
    if isinstance(s, str):
        s = s.encode("utf-8")
    return uleb(len(s)) + s


def section(sid, payload):
    return bytes([sid]) + uleb(len(payload)) + payload


# ------------------------------------------------------------------ s-expression reader

class Str:
    def __init__(self, data):
        self.data = data

    def __repr__(self):
        return f"Str({self.data!r})"


def tokenize(text):
    toks = []
    i, n = 0, len(text)
    while i < n:
        c = text[i]
        if c in " \t\r\n":
            i += 1
        elif text.startswith(";;", i):
            j = text.find("\n", i)
            i = n if j < 0 else j + 1
        elif text.startswith("(;", i):
            j = text.find(";)", i)
            if j < 0:
                raise WatError("unterminated block comment")
            i = j + 2
        elif c in "()":
            toks.append(c)
            i += 1
        elif c == '"':
            i += 1
            buf = bytearray()
            while True:
                if i >= n:
                    raise WatError("unterminated string")
                c = text[i]
                if c == '"':
                    i += 1
                    break
                if c == "\\":
                    e = text[i + 1]
                    esc = {"n": 10, "t": 9, "r": 13, "\\": 92, '"': 34, "'": 39}
                    if e in esc:
                        buf.append(esc[e])
                        i += 2
                    else:
                        buf.append(int(text[i + 1:i + 3], 16))
                        i += 3
                else:
                    buf.extend(c.encode("utf-8"))
                    i += 1
            toks.append(Str(bytes(buf)))
        else:
            j = i
            while j < n and text[j] not in " \t\r\n()":
                j += 1
            toks.append(text[i:j])
            i = j
    return toks


def parse_sexpr(text):
    toks = tokenize(text)
    stack = [[]]
    for t in toks:
        if t == "(":
            stack.append([])
        elif t == ")":
            if len(stack) < 2:
                raise WatError("unbalanced )")
            done = stack.pop()
            stack[-1].append(done)
        else:
            stack[-1].append(t)
    if len(stack) != 1 or len(stack[0]) != 1:
        raise WatError("expected exactly one module")
    return stack[0][0]


def parse_int(s, bits):
    neg = s.startswith("-")
    body = s.lstrip("+-").replace("_", "")
    v = int(body, 16) if body.lower().startswith("0x") else int(body, 10)
    if neg:
        v = -v
    if not (-(1 << (bits - 1)) <= v < (1 << bits)):
        raise WatError(f"constant {s} out of range for {bits} bits")
    if v >= 1 << (bits - 1):
        v -= 1 << bits
    return v


# ------------------------------------------------------------------ assembler

class Module:
    def __init__(self, sexpr):
        if not sexpr or sexpr[0] != "module":
            raise WatError("expected (module ...)")
        self.fields = sexpr[1:]
        self.types = []
        self.type_names = {}
        self.imports = []
        self.func_names = {}
        self.global_names = {}
        self.funcs = []
        self.n_func_imports = 0
        self.n_global_imports = 0

    def type_index(self, params, results):
        key = (tuple(params), tuple(results))
        if key not in self.types:
            self.types.append(key)
        return self.types.index(key)

    def sig(self, items):
        """Reads (type $t)? (param ...)* (result ...)* from items; returns (type idx, names, rest)."""
        params, names, results = [], [], []
        explicit = None
        rest = []
        for it in items:
            if isinstance(it, list) and it and it[0] == "type":
                explicit = self.type_names[it[1]] if it[1].startswith("$") else int(it[1])
            elif isinstance(it, list) and it and it[0] == "param":
                if len(it) == 3 and it[1].startswith("$"):
                    names.append(it[1])
                    params.append(it[2])
                else:
                    for t in it[1:]:
                        names.append(None)
                        params.append(t)
            elif isinstance(it, list) and it and it[0] == "result":
                results.extend(it[1:])
            else:
                rest.append(it)
        if explicit is not None:
            if params or results:
                p, r = self.types[explicit]
                if (tuple(params), tuple(results)) != (p, r):
                    raise WatError("inline signature disagrees with (type)")
            p, _ = self.types[explicit]
            if not names:
                names = [None] * len(p)
            return explicit, names, rest
        return self.type_index(params, results), names, rest

    def assemble(self):
        out_imports, out_funcs, out_exports = [], [], []
        memory = table = start = None
        globals_, elems, datas = [], [], []
        # Pass 1: types and the function and global index spaces.
        for f in self.fields:
            if f[0] == "type":
                fn = f[2] if len(f) == 3 else f[1]
                idx_name = f[1] if len(f) == 3 else None
                p, r = [], []
                for it in fn[1:]:
                    if it[0] == "param":
                        p.extend(x for x in it[1:] if not x.startswith("$"))
                    elif it[0] == "result":
                        r.extend(it[1:])
                self.types.append((tuple(p), tuple(r)))
                if idx_name:
                    self.type_names[idx_name] = len(self.types) - 1
        for f in self.fields:
            if f[0] == "import":
                desc = f[3]
                if desc[0] == "func":
                    items = desc[1:]
                    if items and isinstance(items[0], str) and items[0].startswith("$"):
                        self.func_names[items[0]] = self.n_func_imports
                        items = items[1:]
                    ti, _, _ = self.sig(items)
                    out_imports.append(name(f[1].data) + name(f[2].data) + b"\x00" + uleb(ti))
                    self.n_func_imports += 1
                elif desc[0] == "memory":
                    nums = [int(x) for x in desc[1:] if not x.startswith("$")]
                    out_imports.append(name(f[1].data) + name(f[2].data) + b"\x02" + limits(nums))
                elif desc[0] == "table":
                    nums = [int(x) for x in desc[1:] if x not in ("funcref",) and not x.startswith("$")]
                    out_imports.append(name(f[1].data) + name(f[2].data) + b"\x01\x70" + limits(nums))
                elif desc[0] == "global":
                    t = desc[-1]
                    mut = isinstance(t, list)
                    vt = t[1] if mut else t
                    out_imports.append(name(f[1].data) + name(f[2].data) + b"\x03" + bytes([VALTYPES[vt], int(mut)]))
                    self.n_global_imports += 1
                else:
                    raise WatError(f"unsupported import kind {desc[0]}")
        defined = [f for f in self.fields if f[0] == "func"]
        for k, f in enumerate(defined):
            if len(f) > 1 and isinstance(f[1], str) and f[1].startswith("$"):
                self.func_names[f[1]] = self.n_func_imports + k
        gk = self.n_global_imports
        for f in self.fields:
            if f[0] == "global":
                if isinstance(f[1], str) and f[1].startswith("$"):
                    self.global_names[f[1]] = gk
                gk += 1
        # Pass 2: everything else.
        func_section, code_section = [], []
        for k, f in enumerate(defined):
            items = f[1:]
            if items and isinstance(items[0], str) and items[0].startswith("$"):
                items = items[1:]
            exports = []
            while items and isinstance(items[0], list) and items[0][0] == "export":
                exports.append(items[0][1].data)
                items = items[1:]
            locals_, local_names, body = [], [], []
            head = []
            for it in items:
                if isinstance(it, list) and it and it[0] == "local":
                    if len(it) == 3 and it[1].startswith("$"):
                        local_names.append(it[1])
                        locals_.append(it[2])
                    else:
                        for t in it[1:]:
                            local_names.append(None)
                            locals_.append(t)
                elif isinstance(it, list) and it and it[0] in ("type", "param", "result") and not body:
                    head.append(it)
                else:
                    body.append(it)
            ti, pnames, _ = self.sig(head)
            names = pnames + local_names
            local_map = {n: i for i, n in enumerate(names) if n}
            func_section.append(uleb(ti))
            groups = [uleb(1) + bytes([VALTYPES[t]]) for t in locals_]
            code = vec(groups) + self.code(body, local_map) + b"\x0b"
            code_section.append(uleb(len(code)) + code)
            for e in exports:
                out_exports.append(name(e) + b"\x00" + uleb(self.n_func_imports + k))
        for f in self.fields:
            kind = f[0]
            if kind == "memory":
                nums = [int(x) for x in f[1:] if isinstance(x, str) and not x.startswith("$")]
                memory = limits(nums)
            elif kind == "table":
                nums = [int(x) for x in f[1:] if isinstance(x, str) and x != "funcref" and not x.startswith("$")]
                table = b"\x70" + limits(nums)
            elif kind == "global":
                items = f[2:] if isinstance(f[1], str) and f[1].startswith("$") else f[1:]
                t, init = items[0], items[1]
                mut = isinstance(t, list)
                vt = t[1] if mut else t
                globals_.append(bytes([VALTYPES[vt], int(mut)]) + self.const_expr(init))
            elif kind == "export":
                desc = f[2]
                kinds = {"func": 0, "table": 1, "memory": 2, "global": 3}
                ref = desc[1]
                if desc[0] == "func":
                    idx = self.func_names[ref] if ref.startswith("$") else int(ref)
                elif desc[0] == "global":
                    idx = self.global_names[ref] if ref.startswith("$") else int(ref)
                else:
                    idx = int(ref)
                out_exports.append(name(f[1].data) + bytes([kinds[desc[0]]]) + uleb(idx))
            elif kind == "start":
                start = uleb(self.func_names[f[1]] if f[1].startswith("$") else int(f[1]))
            elif kind == "elem":
                offset = self.const_expr(f[1])
                idxs = [uleb(self.func_names[x] if x.startswith("$") else int(x)) for x in f[2:]]
                elems.append(b"\x00" + offset + vec(idxs))
            elif kind == "data":
                offset = self.const_expr(f[1])
                data = b"".join(x.data for x in f[2:])
                datas.append(b"\x00" + offset + uleb(len(data)) + data)
        types = [b"\x60" + vec([bytes([VALTYPES[t]]) for t in p]) + vec([bytes([VALTYPES[t]]) for t in r])
                 for p, r in self.types]
        out = b"\x00asm\x01\x00\x00\x00"
        if types:
            out += section(1, vec(types))
        if out_imports:
            out += section(2, vec(out_imports))
        if func_section:
            out += section(3, vec(func_section))
        if table is not None:
            out += section(4, vec([table]))
        if memory is not None:
            out += section(5, vec([memory]))
        if globals_:
            out += section(6, vec(globals_))
        if out_exports:
            out += section(7, vec(out_exports))
        if start is not None:
            out += section(8, start)
        if elems:
            out += section(9, vec(elems))
        if code_section:
            out += section(10, vec(code_section))
        if datas:
            out += section(11, vec(datas))
        return out

    def const_expr(self, e):
        op = e[0]
        if op == "i32.const":
            return b"\x41" + sleb(parse_int(e[1], 32)) + b"\x0b"
        if op == "i64.const":
            return b"\x42" + sleb(parse_int(e[1], 64)) + b"\x0b"
        raise WatError(f"unsupported constant expression {e}")

    def code(self, body, local_map):
        out = bytearray()
        labels = []  # innermost last; None for unnamed
        i = 0

        def ref(tok, table, what):
            if tok.startswith("$"):
                if tok not in table:
                    raise WatError(f"unknown {what} {tok}")
                return table[tok]
            return int(tok)

        def label(tok):
            if tok.startswith("$"):
                for depth, lab in enumerate(reversed(labels)):
                    if lab == tok:
                        return depth
                raise WatError(f"unknown label {tok}")
            return int(tok)

        while i < len(body):
            tok = body[i]
            i += 1
            if isinstance(tok, list):
                raise WatError(f"folded instructions are not supported: {tok}")
            if tok in PLAIN:
                out.append(PLAIN[tok])
            elif tok in ("block", "loop", "if"):
                out.append({"block": 0x02, "loop": 0x03, "if": 0x04}[tok])
                lab = None
                if i < len(body) and isinstance(body[i], str) and body[i].startswith("$"):
                    lab = body[i]
                    i += 1
                bt = b"\x40"
                if i < len(body) and isinstance(body[i], list) and body[i][0] == "result":
                    res = body[i][1:]
                    i += 1
                    if len(res) == 1:
                        bt = bytes([VALTYPES[res[0]]])
                    else:
                        bt = sleb(self.type_index([], res))
                out += bt
                labels.append(lab)
            elif tok == "else":
                out.append(0x05)
            elif tok == "end":
                out.append(0x0B)
                labels.pop()
            elif tok in ("br", "br_if"):
                out.append(0x0C if tok == "br" else 0x0D)
                out += uleb(label(body[i]))
                i += 1
            elif tok == "br_table":
                targets = []
                while i < len(body) and isinstance(body[i], str) and (body[i].startswith("$") or body[i].isdigit()):
                    targets.append(label(body[i]))
                    i += 1
                out.append(0x0E)
                out += vec([uleb(t) for t in targets[:-1]]) + uleb(targets[-1])
            elif tok == "call":
                out.append(0x10)
                out += uleb(ref(body[i], self.func_names, "function"))
                i += 1
            elif tok == "call_indirect":
                items = []
                while i < len(body) and isinstance(body[i], list) and body[i][0] in ("type", "param", "result"):
                    items.append(body[i])
                    i += 1
                ti, _, _ = self.sig(items)
                out.append(0x11)
                out += uleb(ti) + b"\x00"
            elif tok in ("local.get", "local.set", "local.tee"):
                out.append({"local.get": 0x20, "local.set": 0x21, "local.tee": 0x22}[tok])
                out += uleb(ref(body[i], local_map, "local"))
                i += 1
            elif tok in ("global.get", "global.set"):
                out.append(0x23 if tok == "global.get" else 0x24)
                out += uleb(ref(body[i], self.global_names, "global"))
                i += 1
            elif tok in LOADS or tok in STORES:
                out.append(0x28 + LOADS.index(tok) if tok in LOADS else 0x36 + STORES.index(tok))
                offset, align = 0, NATURAL[tok]
                while i < len(body) and isinstance(body[i], str) and body[i].split("=")[0] in ("offset", "align"):
                    k, v = body[i].split("=")
                    if k == "offset":
                        offset = int(v, 0)
                    else:
                        align = int(v, 0).bit_length() - 1
                    i += 1
                out += uleb(align) + uleb(offset)
            elif tok in ("memory.size", "memory.grow"):
                out += bytes([0x3F if tok == "memory.size" else 0x40, 0x00])
            elif tok == "i32.const":
                out.append(0x41)
                out += sleb(parse_int(body[i], 32))
                i += 1
            elif tok == "i64.const":
                out.append(0x42)
                out += sleb(parse_int(body[i], 64))
                i += 1
            elif tok in SAT:
                out += b"\xfc" + uleb(SAT.index(tok))
            elif tok == "memory.copy":
                out += b"\xfc" + uleb(10) + b"\x00\x00"
            elif tok == "memory.fill":
                out += b"\xfc" + uleb(11) + b"\x00"
            else:
                raise WatError(f"unknown instruction {tok}")
        if labels:
            raise WatError("unbalanced block/end")
        return bytes(out)


def limits(nums):
    if len(nums) == 1:
        return b"\x00" + uleb(nums[0])
    if len(nums) == 2:
        return b"\x01" + uleb(nums[0]) + uleb(nums[1])
    raise WatError(f"bad limits {nums}")


def assemble(text):
    """Assembles WAT source text into WebAssembly binary bytes."""
    return Module(parse_sexpr(text)).assemble()
