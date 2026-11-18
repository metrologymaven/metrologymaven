# Verifying a download

This guide checks a downloaded Metrology Maven file offline, with tools that come with
Windows, Linux and macOS. CI runs the command blocks below exactly as written, on all three
systems, against a small fixture release (`fixtures/rel/guide/release/`), so they stay correct.

## What you can check today, and what you cannot

**No release or nightly build is signed yet, and no public key is published** (see
[KEYS.md](KEYS.md)). Today you can check one thing: that your file matches the SHA-256 hash
listed for it in the release's `SHA256SUMS` file. That catches a damaged or incomplete
download. It does **not** prove who made the file: anyone who can change the file on the
download site can change `SHA256SUMS` too. The signature check that closes this gap is
described under [Step 2](#step-2-check-the-signature-on-sha256sums-planned) and is marked
planned until the project publishes its keys.

**Nightly builds are not releases.** When signing exists, nightly builds will be signed with a
separate, lower-trust nightly key by unattended CI. Never treat a nightly build, or anything
signed only by the nightly key, as a release.

## Before you start

Put the file you downloaded and the release's `SHA256SUMS` in the same folder, and open a
terminal in that folder (Windows: PowerShell; macOS: Terminal; Linux: any shell). You do not
need the other files of the release: the commands check every listed file that is present
and ignore the rest. Nothing below uses the network.

## Step 1: check the file against SHA256SUMS

### Windows (Windows PowerShell)

```powershell verify-windows
$checked = 0; $failed = 0
foreach ($line in Get-Content -LiteralPath SHA256SUMS) {
    if ($line -cnotmatch '^([0-9a-f]{64})  ([A-Za-z0-9._+-]+)$') { Write-Output "BAD LINE: $line"; $failed++; continue }
    $expected = $Matches[1]; $name = $Matches[2]
    if (-not (Test-Path -LiteralPath $name -PathType Leaf)) { continue }
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $name).Hash.ToLowerInvariant()
    if ($actual -ceq $expected) { Write-Output "${name}: OK" } else { Write-Output "${name}: FAILED"; $failed++ }
    $checked++
}
if ($failed -gt 0 -or $checked -eq 0) { throw "Verification FAILED ($failed failed, $checked checked)" }
```

Every file you downloaded must print `OK`. A red `Verification FAILED` error means a file is
damaged or is not the released file: delete it and download it again.

### Linux

```sh verify-linux
sha256sum --strict --ignore-missing -c SHA256SUMS
```

Every file you downloaded must print `OK`. Any `FAILED` line, or `no file was verified`,
means the check failed.

### macOS

```sh verify-macos
shasum -a 256 --strict --ignore-missing -c SHA256SUMS
```

Every file you downloaded must print `OK`. Any `FAILED` line means the check failed.

## Step 2: check the signature on SHA256SUMS (planned)

**Not available yet: no key exists and `SHA256SUMS.sig` is not published.** Once the release
key is published in `keys/allowed_signers` and listed in [KEYS.md](KEYS.md), releases will
include `SHA256SUMS.sig`, an SSH signature over `SHA256SUMS` in namespace `fid-release`, and
this step will become a tested command block for each system. The planned command, identical
on all three systems (OpenSSH 8.0 or later provides `ssh-keygen -Y`), is:

```text
ssh-keygen -Y verify -f allowed_signers -I <signer identity from KEYS.md> -n fid-release -s SHA256SUMS.sig < SHA256SUMS
```

On Windows, `ssh-keygen` comes with the optional OpenSSH Client feature; whether it is present
by default on every supported Windows version is not yet confirmed [unverified]. Windows
PowerShell 5.1 has no `<` redirection, so the Windows form will differ.

## For maintainers

- `SHA256SUMS` is written by `cargo xtask rel sums` (`ECO.REL.SIGN.SUMS`).
- The command blocks tagged `verify-windows`, `verify-linux` and `verify-macos` are extracted
  and run by `python tools/verify_guide.py` in the `verify-guide` CI job, which also checks
  that a file with one changed byte fails. Keep every runnable command inside those blocks.
