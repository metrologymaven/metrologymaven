---
id: D-030
title: Principal kinds for callers authenticated by the self-hosted server
status: accepted
raised_by: ENT split
blocks: [ENT.IAM]
decide_by: before ENT.IAM is split into modules
updated: 2026-11-17
---

# D-030: Principal kinds for callers authenticated by the self-hosted server

## Question

When a caller reaches the optional self-hosted server (ENT) through an OIDC or SAML sign-in, an LDAP-synced account or a service credential, which channel kind and principal kind (`human`, `agent`, `script`, `service`) does the server's command bus assign? In particular, can a person signed in through the server's web client ever be `human`, and so perform W3 (release) on server-held records?

## Context

REQ-ENT-002 requires the server to map identity-provider users to principals and roles, with principals taken only from the authenticated channel (CONSTITUTION §3). `CORE.BUS.PRIN` defines a closed, exhaustive mapping from channel kind to principal kind (gui to human, cli/python/control to script, mcp to agent, service, device, internal to system). It has no channel kind for a remote, network-authenticated session, so ENT cannot add one without a change to `CORE.BUS.PRIN` (and that node is itself waiting on D-012).

The cases are:
- a person in the self-hosted web client (WASM build) after an interactive IdP sign-in;
- a desktop app synchronising on behalf of its signed-in user (ENT.SYNC);
- connectors and webhook delivery (ENT.CONNECT, ENT.EVENTS) running under service credentials;
- an agent reaching server tools over MCP.

An IdP sign-in can carry evidence of fresh interactive authentication (authentication time, MFA context) that a local CLI cannot [unverified for every IdP]. It is still a bearer token once issued, and a token copied to an agent's environment is indistinguishable from the person.

Affected nodes: `ENT.IAM` (blocked for the principal-mapping module), `ENT.SRV` (W3 release of server-held programs and templates), `ENT.SYNC`, `CORE.BUS.PRIN`, `CORE.JRN.STREAM` (Access Ledger pseudonyms for external principals). Related: D-012.

## Options

| Option | For | Against |
|---|---|---|
| A. New channel kind `server_session`: `human` only while the IdP assertion shows interactive authentication within a configured window (default 15 min [unverified]) and the request comes from the web client; otherwise `script` with the user as `on_behalf_of`. Service credentials map to `service`; server MCP to `agent` | Lets people release on the server; uses evidence the IdP already gives | Adds a channel kind to a closed enum; trust in token freshness; IdP-specific claims |
| B. Every server caller except service credentials is `script` with `on_behalf_of`; W3 on server-held records happens only by pushing a release made in a desktop app (GUI `human`) | No change to CORE.BUS.PRIN semantics; one place for releases | Web client is read-mostly; releasing a program library entry needs a desktop |
| C. Server web client is `human` like the desktop GUI, with no freshness rule | Simplest for users | A stolen or delegated token acts as a human; weakest against agents with token access |

## Recommendation

B for the first server release, because it keeps release and signing (S1) on the desktop where `TRUST.ESIG` and the OS keystore already live, and needs no change to `CORE.BUS.PRIN`. Revisit A once D-012 is decided, since both questions are about proving human presence outside the desktop GUI.

## Decision

**Accepted (recommendation as written above), 2026-11-17, founder.**

B for the first server release, because it keeps release and signing (S1) on the desktop where `TRUST.ESIG` and the OS keystore already live, and needs no change to `CORE.BUS.PRIN`. Revisit A once D-012 is decided, since both questions are about proving human presence outside the desktop GUI.
