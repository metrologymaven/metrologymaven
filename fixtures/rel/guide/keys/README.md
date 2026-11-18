# Fixture keys (FAKE)

These are not keys. Each PEM file wraps 32 bytes of ASCII text in an Ed25519
SubjectPublicKeyInfo structure so that `cargo xtask rel keys-check` has something to
fingerprint. No private key exists for any of them. Written by ../make_fixtures.py.
