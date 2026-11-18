# Public keys

This directory holds the project's public keys: `<name>.pem` (SubjectPublicKeyInfo PEM) and
`allowed_signers` (OpenSSH format). **It holds no keys yet**: no signing key has been created.
Every key file placed here must be listed in [../KEYS.md](../KEYS.md); CI runs
`cargo xtask rel keys-check` to enforce it. Private keys never go in this repository.
