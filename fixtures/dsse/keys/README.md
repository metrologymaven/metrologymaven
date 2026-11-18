# TEST KEYS ONLY. Do not trust, do not reuse.

The private keys in this folder are published test data, not secrets:

- `ed25519-test.pem`: the RFC 8032 section 7.1 TEST 1 Ed25519 seed, as PKCS#8.
- `p256-test.pem`: the RFC 6979 appendix A.2.5 P-256 private key, as PKCS#8.
- `rsa-test.pub.pem`: a structurally valid RSA public key around a made-up modulus. No
  private key exists for it; it only exercises `sig.unsupported_algorithm`.

Anyone can sign with these keys. Nothing signed with them proves anything. They exist so
that `fid-sign-core` tests (TRUST.SIGN.DSSE) are deterministic. All files here, and the
envelopes in `../golden/`, are written by `../make_fixtures.py`.
