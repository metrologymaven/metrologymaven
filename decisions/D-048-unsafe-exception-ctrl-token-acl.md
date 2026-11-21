---
id: D-048
title: Named unsafe exception for the control-channel token-file ACL shim
status: accepted
raised_by: build of AUTO.CTRL.AUTH (PR #410), review of the Windows token-file DACL
blocks: [AUTO.CTRL.AUTH]
decide_by: before PR #410 merges
updated: 2026-11-21
---

# D-048: Named unsafe exception for the control-channel token-file ACL shim

**Status: ACCEPTED 2026-11-21** (maintainer decision): one scoped exception, gated by a CI test.

## Question

May one crate use `unsafe` so that the control-channel token file gets, and is checked for, an
owner-only DACL on Windows (REQ-AUTO.CTRL.AUTH-001, -002)? If so, within which bounds?

## Context

- CONSTITUTION §8 forbids `unsafe` in non-test code, except for named exceptions in non-kernel
  crates. The workspace sets `unsafe_code = "forbid"`, which a crate cannot lift with `allow`.
- AUTO.CTRL.AUTH needs to do three things on Windows. It creates the token file atomically
  with a protected DACL that grants access only to the user's SID, and with that SID as owner.
  It reads the owner and DACL back from an open handle. It reads the process user's SID. The
  AUTO.CTRL.AUTH build prompt anticipates this: "no `unsafe` beyond the OS ACL shim (named
  exception)".
- std has no ACL or security-descriptor API. The Win32 calls involved (`CreateFileW` with
  `SECURITY_ATTRIBUTES`, `GetSecurityInfo`, `InitializeAcl`, `AddAccessAllowedAce`,
  `GetAce`, `OpenProcessToken`, `GetTokenInformation`) are exposed by windows-rs
  (`windows-sys`) only as `unsafe fn`. No maintained crate on the licence allowlist wraps
  them safely.

## Options

| Option | For | Against |
|---|---|---|
| A: shim crate with `unsafe` in one module | Exactly what REQ-001/-002 ask; checks run on the handle, so there is no path race | First `unsafe` in the workspace |
| B: shell out to `icacls` / PowerShell | No `unsafe` | Creation is not atomic; output depends on the locale; a process per authentication; not "the OS API" |
| C: rely on the inherited ACL of the per-user directory | No `unsafe` | Cannot satisfy REQ-001 (protected DACL) or detect REQ-002 on Windows |

## Decision

Option A, bounded as follows.

1. **Scope.** Only the Windows token-file security: creation with an owner-only protected
   DACL, reading back a handle's owner and DACL (for the token file and for its directory),
   reading the process user's SID, and reading a file's identity (volume serial number and
   file index). It lives in the crate `fid-ctrl-acl`, module `ffi`, and nowhere else; `ffi`
   only copies data out of the OS, and every decision is made in safe code. The crate is empty
   on every other target.
2. **Reason.** std has no ACL API, and windows-rs exposes these calls only as `unsafe fn`.
3. **Lints.** `fid-ctrl-acl` is the one workspace member without `[lints] workspace = true`.
   Its own `[lints]` are the workspace's with exactly two changes: `rust.unsafe_code` is
   `deny` (instead of `forbid`), lifted only on `mod ffi`, and `clippy` adds
   `undocumented_unsafe_blocks = "deny"`. No other lint may be added, dropped or changed, and
   the crate has no build script.
4. **Rule.** No other crate may follow this precedent without its own decision record. A new
   `unsafe` site in `fid-ctrl-acl` outside `mod ffi` also needs one.
5. **Gate.** `cargo test -p fid-xtask --test unsafe_exception` fails when:
   - a workspace member (every `workspace.members` entry, globs expanded, so
     `vendor/fid-clarabel/crate` too) other than `fid-ctrl-acl` lacks
     `[lints] workspace = true`;
   - the exception crate's lint tables differ from rule 3, or it has a build script;
   - any `.rs` file in the repository has an attribute naming `unsafe_code` that is not a
     plain `deny`/`forbid`, other than one `#[allow(unsafe_code)]` directly on the top-level
     `mod ffi { .. }` of `crates/fid-ctrl-acl/src/lib.rs`. Files are tokenised (proc-macro2,
     the lexer under syn), so attributes over several lines, behind `cfg_attr`, nested in
     items or built by macros are caught, and comments and strings are not. The ident
     `unsafe_code` outside an attribute also fails. A short, named list of fixtures that are
     deliberately not Rust tokens gets a text check instead;
   - in `fid-ctrl-acl`, an attribute lifts `clippy::undocumented_unsafe_blocks` or the
     `restriction` group, or a `#[path]` attribute or `include!` pulls in another file.

   The gate tests itself on each of these bypasses.

## Consequences

- `fid-ctrl` keeps `forbid(unsafe_code)` and calls only the safe functions of `fid-ctrl-acl`.
- Every change to `fid-ctrl-acl` needs an independent review (D-037 P2) that checks each
  `// SAFETY:` comment.
- Kernel crates (KNP-1) stay without any exception.
