#!/usr/bin/env python3
"""Independent reference for the TRUST.SIGN.DSSE fixtures (fid-sign-core).

Stdlib only. Nothing here runs or imports the Rust code under test. It contains its own
small implementations of:

- DSSE v1.0 pre-authentication encoding (PAE):
  "DSSEv1" SP LEN(type) SP type SP LEN(body) SP body, LEN = ASCII decimal byte count;
- Ed25519 signing (RFC 8032 section 5.1), checked first against RFC 8032 section 7.1
  TEST 1 and TEST 2 (keys, messages and signatures typed in as hex test data);
- ECDSA P-256 / SHA-256 signing with deterministic nonces (RFC 6979 section 3.2), checked
  first against the RFC 6979 appendix A.2.5 P-256 key and its SHA-256 "sample" signature;
- SubjectPublicKeyInfo (RFC 5280, RFC 5480, RFC 8410) and PKCS#8 (RFC 5208) DER encoders,
  and RFC 8785-style compact JSON (sorted keys, no whitespace, no trailing newline).

TEST KEYS ONLY. The two private keys written to keys/ are the published RFC 8032 TEST 1
Ed25519 seed and the RFC 6979 A.2.5 P-256 private key. They are public knowledge and must
never protect anything. The RSA public key is structurally valid DER around a made-up
modulus; no private key exists for it.

Run:  python fixtures/dsse/make_fixtures.py          (write)
      python fixtures/dsse/make_fixtures.py --check  (fail if any file would change)
"""

import base64
import hashlib
import hmac
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
PAYLOAD_TYPE = "application/vnd.in-toto+json"


# ---------------------------------------------------------------- PAE


def pae(payload_type: bytes, body: bytes) -> bytes:
    out = b"DSSEv1 " + str(len(payload_type)).encode("ascii") + b" " + payload_type
    out += b" " + str(len(body)).encode("ascii") + b" " + body
    return out


# ---------------------------------------------------------------- Ed25519 (RFC 8032)

EP = 2**255 - 19
EL = 2**252 + 27742317777372353535851937790883648493
ED = (-121665 * pow(121666, EP - 2, EP)) % EP
SQRT_M1 = pow(2, (EP - 1) // 4, EP)


def ed_recover_x(y, sign):
    x2 = (y * y - 1) * pow(ED * y * y + 1, EP - 2, EP) % EP
    if x2 == 0:
        return None if sign else 0
    x = pow(x2, (EP + 3) // 8, EP)
    if (x * x - x2) % EP != 0:
        x = x * SQRT_M1 % EP
    if (x * x - x2) % EP != 0:
        return None
    if (x & 1) != sign:
        x = EP - x
    return x


EBY = 4 * pow(5, EP - 2, EP) % EP
EBX = ed_recover_x(EBY, 0)
EB = (EBX, EBY, 1, EBX * EBY % EP)
EZERO = (0, 1, 1, 0)


def ed_add(p, q):
    x1, y1, z1, t1 = p
    x2, y2, z2, t2 = q
    a = (y1 - x1) * (y2 - x2) % EP
    b = (y1 + x1) * (y2 + x2) % EP
    c = 2 * t1 * t2 * ED % EP
    d = 2 * z1 * z2 % EP
    e, f, g, h = b - a, d - c, d + c, b + a
    return (e * f % EP, g * h % EP, f * g % EP, e * h % EP)


def ed_mul(k, p):
    acc = EZERO
    while k:
        if k & 1:
            acc = ed_add(acc, p)
        p = ed_add(p, p)
        k >>= 1
    return acc


def ed_encode(p):
    x, y, z, _ = p
    zi = pow(z, EP - 2, EP)
    x, y = x * zi % EP, y * zi % EP
    return (y | ((x & 1) << 255)).to_bytes(32, "little")


def ed_secret(seed):
    h = hashlib.sha512(seed).digest()
    a = int.from_bytes(h[:32], "little")
    a &= (1 << 254) - 8
    a |= 1 << 254
    return a, h[32:]


def ed_public(seed):
    a, _ = ed_secret(seed)
    return ed_encode(ed_mul(a, EB))


def ed_sign(seed, msg):
    a, prefix = ed_secret(seed)
    pub = ed_encode(ed_mul(a, EB))
    r = int.from_bytes(hashlib.sha512(prefix + msg).digest(), "little") % EL
    big_r = ed_encode(ed_mul(r, EB))
    k = int.from_bytes(hashlib.sha512(big_r + pub + msg).digest(), "little") % EL
    s = (r + k * a) % EL
    return big_r + s.to_bytes(32, "little")


# RFC 8032 section 7.1, TEST 1 and TEST 2 (test data typed as hex).
RFC8032 = [
    {
        "name": "rfc8032-test1",
        "secret": "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
        "public": "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
        "message": "",
        "signature": "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
    },
    {
        "name": "rfc8032-test2",
        "secret": "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb",
        "public": "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
        "message": "72",
        "signature": "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
    },
]

# ---------------------------------------------------------------- P-256 (FIPS 186, RFC 6979)

PP = 2**256 - 2**224 + 2**192 + 2**96 - 1
PA = PP - 3
PB = 0x5AC635D8AA3A93E7B3EBBD55769886BC651D06B0CC53B0F63BCE3C3E27D2604B
PN = 0xFFFFFFFF00000000FFFFFFFFFFFFFFFFBCE6FAADA7179E84F3B9CAC2FC632551
PG = (
    0x6B17D1F2E12C4247F8BCE6E563A440F277037D812DEB33A0F4A13945D898C296,
    0x4FE342E2FE1A7F9B8EE7EB4A7C0F9E162BCE33576B315ECECBB6406837BF51F5,
)


def p_on_curve(pt):
    x, y = pt
    return (y * y - (x * x * x + PA * x + PB)) % PP == 0


def p_add(p, q):
    if p is None:
        return q
    if q is None:
        return p
    if p[0] == q[0] and (p[1] + q[1]) % PP == 0:
        return None
    if p == q:
        lam = (3 * p[0] * p[0] + PA) * pow(2 * p[1], PP - 2, PP) % PP
    else:
        lam = (q[1] - p[1]) * pow(q[0] - p[0], PP - 2, PP) % PP
    x = (lam * lam - p[0] - q[0]) % PP
    return (x, (lam * (p[0] - x) - p[1]) % PP)


def p_mul(k, p):
    acc = None
    while k:
        if k & 1:
            acc = p_add(acc, p)
        p = p_add(p, p)
        k >>= 1
    return acc


def rfc6979_k(x, h1):
    """RFC 6979 section 3.2 with HMAC-SHA-256, qlen = hlen = 256."""
    xo = x.to_bytes(32, "big")
    ho = (int.from_bytes(h1, "big") % PN).to_bytes(32, "big")
    v = b"\x01" * 32
    k = b"\x00" * 32
    k = hmac.new(k, v + b"\x00" + xo + ho, hashlib.sha256).digest()
    v = hmac.new(k, v, hashlib.sha256).digest()
    k = hmac.new(k, v + b"\x01" + xo + ho, hashlib.sha256).digest()
    v = hmac.new(k, v, hashlib.sha256).digest()
    while True:
        v = hmac.new(k, v, hashlib.sha256).digest()
        cand = int.from_bytes(v, "big")
        if 1 <= cand < PN:
            return cand
        k = hmac.new(k, v + b"\x00", hashlib.sha256).digest()
        v = hmac.new(k, v, hashlib.sha256).digest()


def p_sign(x, msg):
    """Returns (r, s) without low-S normalisation."""
    h1 = hashlib.sha256(msg).digest()
    e = int.from_bytes(h1, "big")
    k = rfc6979_k(x, h1)
    r = p_mul(k, PG)[0] % PN
    s = pow(k, PN - 2, PN) * (e + r * x) % PN
    assert r != 0 and s != 0
    return r, s


def low_s(s):
    return PN - s if s > PN // 2 else s


def p_public(x):
    pt = p_mul(x, PG)
    return b"\x04" + pt[0].to_bytes(32, "big") + pt[1].to_bytes(32, "big")


# RFC 6979 appendix A.2.5 (P-256): private key, public key, and the SHA-256 signature
# over the ASCII message "sample" (test data typed as hex).
RFC6979_X = 0xC9AFA9D845BA75166B5C215767B1D6934E50C3DB36E89B127B8A622B120F6721
RFC6979_UX = 0x60FED4BA255A9D31C961EB74C6356D68C049B8923B61FA6CE669622E60F29FB6
RFC6979_UY = 0x7903FE1008B8BC99A41AE9E95628BC64F2F1B20C2D7E9F5177A3C294D4462299
RFC6979_SAMPLE_R = 0xEFD48B2AACB6A8FD1140DD9CD45E81D69D2C877B56AAF991C34D0EA84EAF3716
RFC6979_SAMPLE_S = 0xF7CB1C942D657C41D436C7A1B6E29F65F3E900DBB9AFF4064DC4AB2F843ACDA8

# ---------------------------------------------------------------- DER


def der_len(n):
    if n < 0x80:
        return bytes([n])
    b = n.to_bytes((n.bit_length() + 7) // 8, "big")
    return bytes([0x80 | len(b)]) + b


def tlv(tag, content):
    return bytes([tag]) + der_len(len(content)) + content


def der_int(v):
    b = v.to_bytes(max(1, (v.bit_length() + 7) // 8), "big")
    if b[0] & 0x80:
        b = b"\x00" + b
    return tlv(0x02, b)


def seq(*parts):
    return tlv(0x30, b"".join(parts))


OID_ED25519 = tlv(0x06, bytes.fromhex("2b6570"))
OID_EC_PUBLIC_KEY = tlv(0x06, bytes.fromhex("2a8648ce3d0201"))
OID_PRIME256V1 = tlv(0x06, bytes.fromhex("2a8648ce3d030107"))
OID_RSA = tlv(0x06, bytes.fromhex("2a864886f70d010101"))
DER_NULL = bytes.fromhex("0500")


def spki_ed25519(pub):
    return seq(seq(OID_ED25519), tlv(0x03, b"\x00" + pub))


def spki_p256(pub65):
    return seq(seq(OID_EC_PUBLIC_KEY, OID_PRIME256V1), tlv(0x03, b"\x00" + pub65))


def pkcs8_ed25519(seed):
    return seq(der_int(0), seq(OID_ED25519), tlv(0x04, tlv(0x04, seed)))


def pkcs8_p256(x):
    ec = seq(der_int(1), tlv(0x04, x.to_bytes(32, "big")), tlv(0xA1, tlv(0x03, b"\x00" + p_public(x))))
    return seq(der_int(0), seq(OID_EC_PUBLIC_KEY, OID_PRIME256V1), tlv(0x04, ec))


def ecdsa_der(r, s):
    return seq(der_int(r), der_int(s))


def pem(label, der):
    b = base64.b64encode(der).decode("ascii")
    lines = [b[i : i + 64] for i in range(0, len(b), 64)]
    return "-----BEGIN %s-----\n%s\n-----END %s-----\n" % (label, "\n".join(lines), label)


def fingerprint(spki):
    return hashlib.sha256(spki).hexdigest()


# ---------------------------------------------------------------- JSON


def jcs(obj):
    """Compact JSON with sorted keys; equals RFC 8785 for the ASCII-only data used here."""
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def envelope(payload, payload_type, sigs):
    return jcs(
        {
            "payload": base64.b64encode(payload).decode("ascii"),
            "payloadType": payload_type,
            "signatures": [{"keyid": k, "sig": base64.b64encode(s).decode("ascii")} for k, s in sigs],
        }
    )


def chain(label, n):
    out = b""
    i = 0
    while len(out) < n:
        out += hashlib.sha256(label + i.to_bytes(4, "big")).digest()
        i += 1
    return out[:n]


# ---------------------------------------------------------------- self-checks


def self_check():
    for t in RFC8032:
        seed = bytes.fromhex(t["secret"])
        msg = bytes.fromhex(t["message"])
        assert ed_public(seed).hex() == t["public"], t["name"] + " public"
        assert ed_sign(seed, msg).hex() == t["signature"], t["name"] + " signature"
    assert p_on_curve(PG)
    assert p_mul(PN, PG) is None
    pub = p_public(RFC6979_X)
    assert pub == b"\x04" + RFC6979_UX.to_bytes(32, "big") + RFC6979_UY.to_bytes(32, "big"), "rfc6979 public"
    assert p_sign(RFC6979_X, b"sample") == (RFC6979_SAMPLE_R, RFC6979_SAMPLE_S), "rfc6979 sample"
    assert pae(b"http://example.com/HelloWorld", b"hello world") == (
        b"DSSEv1 29 http://example.com/HelloWorld 11 hello world"
    )


# ---------------------------------------------------------------- fixtures


def pae_vectors():
    cases = [
        ("empty", b"", b""),
        ("example-hello-world", b"http://example.com/HelloWorld", b"hello world"),
        ("in-toto-type", PAYLOAD_TYPE.encode(), b'{"_type":"x"}'),
        ("spaces-in-type-and-body", b"a b c", b"  "),
        ("binary-body", b"application/octet-stream", bytes(range(256))),
        ("non-ascii-type-utf8-length", "tést/ü".encode("utf-8"), b"\xff\x00\n\r"),
        ("ten-byte-body", b"t", b"0123456789"),
        ("long-body-1000", b"text/plain", chain(b"pae-long", 1000)),
        ("digits-in-type", b"10", b"10"),
    ]
    out = []
    for name, t, body in cases:
        out.append(
            {
                "name": name,
                "payloadType_hex": t.hex(),
                "payload_hex": body.hex(),
                "pae_hex": pae(t, body).hex(),
            }
        )
    return out


def build():
    files = {}
    ed_seed = bytes.fromhex(RFC8032[0]["secret"])
    ed_pub = ed_public(ed_seed)
    ed_spki = spki_ed25519(ed_pub)
    p_x = RFC6979_X
    p_pub = p_public(p_x)
    p_spki = spki_p256(p_pub)
    rsa_n = int.from_bytes(chain(b"rsa-test-modulus-not-a-real-key", 256), "big") | (1 << 2047) | 1
    rsa_spki = seq(seq(OID_RSA, DER_NULL), tlv(0x03, b"\x00" + seq(der_int(rsa_n), der_int(65537))))

    files["keys/ed25519-test.pem"] = pem("PRIVATE KEY", pkcs8_ed25519(ed_seed)).encode()
    files["keys/ed25519-test.pub.pem"] = pem("PUBLIC KEY", ed_spki).encode()
    files["keys/ed25519-test.fingerprint"] = (fingerprint(ed_spki) + "\n").encode()
    files["keys/p256-test.pem"] = pem("PRIVATE KEY", pkcs8_p256(p_x)).encode()
    files["keys/p256-test.pub.pem"] = pem("PUBLIC KEY", p_spki).encode()
    files["keys/p256-test.fingerprint"] = (fingerprint(p_spki) + "\n").encode()
    files["keys/rsa-test.pub.pem"] = pem("PUBLIC KEY", rsa_spki).encode()
    files["keys/rsa-test.fingerprint"] = (fingerprint(rsa_spki) + "\n").encode()

    statement = jcs(
        {
            "_type": "https://in-toto.io/Statement/v1",
            "subject": [
                {
                    "name": "result-package.fidpkg",
                    "digest": {"sha256": hashlib.sha256(b"fid-sign-core test subject").hexdigest()},
                }
            ],
            "predicateType": "https://example.invalid/fid/result-package/v0",
            "predicate": {"note": "TEST FIXTURE: signed with published RFC test keys"},
        }
    )
    files["statement.json"] = statement
    m = pae(PAYLOAD_TYPE.encode(), statement)

    files["golden/ed25519.dsse.json"] = envelope(statement, PAYLOAD_TYPE, [(fingerprint(ed_spki), ed_sign(ed_seed, m))])

    r, s = p_sign(p_x, m)
    s_low = low_s(s)
    s_high = PN - s_low
    p_fp = fingerprint(p_spki)
    files["golden/p256.dsse.json"] = envelope(statement, PAYLOAD_TYPE, [(p_fp, ecdsa_der(r, s_low))])
    files["golden/p256-high-s.dsse.json"] = envelope(statement, PAYLOAD_TYPE, [(p_fp, ecdsa_der(r, s_high))])
    # Non-canonical DER of the valid low-S signature: an extra leading zero on r, and the
    # SEQUENCE length in long form. Both decode to the same (r, s) under lax BER.
    r_bytes = der_int(r)[2:]
    padded_r = tlv(0x02, b"\x00" + r_bytes)
    files["golden/p256-nonminimal-int.dsse.json"] = envelope(
        statement, PAYLOAD_TYPE, [(p_fp, tlv(0x30, padded_r + der_int(s_low)))]
    )
    body = der_int(r) + der_int(s_low)
    files["golden/p256-longform-len.dsse.json"] = envelope(
        statement, PAYLOAD_TYPE, [(p_fp, b"\x30\x81" + bytes([len(body)]) + body)]
    )
    files["golden/rsa.dsse.json"] = envelope(
        statement, PAYLOAD_TYPE, [(fingerprint(rsa_spki), chain(b"rsa-test-signature-not-real", 256))]
    )
    files["golden/two-signatures.dsse.json"] = envelope(
        statement,
        PAYLOAD_TYPE,
        [(p_fp, ecdsa_der(r, s_low)), (fingerprint(ed_spki), ed_sign(ed_seed, m))],
    )

    files["vectors/pae.json"] = jcs(pae_vectors())
    files["vectors/ed25519-rfc8032.json"] = jcs(RFC8032)
    files["vectors/p256-rfc6979.json"] = jcs(
        [
            {
                "name": "rfc6979-a.2.5-sha256-sample",
                "public_sec1": p_pub.hex(),
                "message": b"sample".hex(),
                "low_s_der": ecdsa_der(RFC6979_SAMPLE_R, low_s(RFC6979_SAMPLE_S)).hex(),
                "high_s_der": ecdsa_der(RFC6979_SAMPLE_R, RFC6979_SAMPLE_S).hex(),
            }
        ]
    )
    return files


def main():
    self_check()
    check = "--check" in sys.argv[1:]
    stale = []
    for rel, data in sorted(build().items()):
        path = HERE / rel
        if check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(rel)
            continue
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    if stale:
        print("stale fixtures: " + ", ".join(stale))
        return 1
    print("fixtures/dsse: %s" % ("up to date" if check else "written"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
