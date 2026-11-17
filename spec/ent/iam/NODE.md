---
id: ENT.IAM
title: Identity and access: SSO, directory sync, roles and principal mapping
level: 2
parent: ENT
kind: subsystem
status: open
phase: P1b
size: XL
owners: []
claimed_by:
satisfies: [REQ-ENT-002]
covers: []
depends_on: [CORE.BUS.PRIN, CORE.JRN.STREAM, TRUST.PROFILE.GATE]
cli: [fid server *-user, fid server *-role, fid server *-idp, fid server *-directory, fid server *-login, fid server *-credential]
mcp: [server_list_users, server_show_user, server_list_roles, server_show_role]
journal: [server.*_user@*, server.*_role@*, server.*_idp@*, server.*_directory@*, server.*_credential@*]
updated: 2026-11-16
---

# ENT.IAM: Identity and access: SSO, directory sync, roles and principal mapping

## Purpose

Who is calling the server and what they may do. It authenticates people through OIDC or SAML, keeps accounts in step with an LDAP directory, issues and revokes service credentials, defines roles and site scopes, and turns an authenticated identity into a bus principal on the server. It is separate because the rule "principals come only from the authenticated channel" (CONSTITUTION §3) is enforced in one place for all server traffic.

## Scope

**In:**
- OIDC and SAML sign-in against an organisation's identity provider; desktop sign-in to a server (sign-in flow named at leaf level)
- LDAP directory sync of users and groups into server accounts
- Roles, role-to-permission sets, group-to-role mapping, and site scopes
- Mapping roles to governance profile expectations (`TRUST.PROFILE`)
- Service credentials for connectors and webhook delivery, with rotation and revocation
- Server channel definition and the identity-to-principal mapping (blocked on D-030)
- Sign-in, denial and credential events in the Access Ledger with pseudonymised identities (`CORE.JRN.STREAM` format)

**Out** (and which node owns it instead):
- Enforcing a role check inside each server command (`ENT.SRV`)
- Local desktop principals and the channel-kind enum (`CORE.BUS.PRIN`)
- Signing keys, user verification and e-signature ceremonies (`TRUST.SIGN`, `TRUST.ESIG`)
- The identity registry mapping pseudonyms to people for journals (`TRUST`)

## Requirements

- **REQ-ENT.IAM-001** The server shall authenticate people through OIDC and through SAML 2.0, and shall refuse every server command from a caller with no valid authenticated session or service credential with a typed `unauthenticated` error.
- **REQ-ENT.IAM-002** The server shall take a caller's principal kind and identity only from the authenticated session or credential, and shall ignore any principal, role or on-behalf-of field in the request.
- **REQ-ENT.IAM-003** When the IdP assertion or token fails signature, audience, issuer or expiry validation, then the server shall refuse the sign-in and record the refusal in the Access Ledger.
- **REQ-ENT.IAM-004** When an LDAP directory sync runs, the server shall create, update and disable accounts and group memberships to match the directory, and a user removed from the directory shall lose server access within one sync interval.
- **REQ-ENT.IAM-005** Creating, changing or deleting a role, role mapping, identity-provider configuration, directory configuration or service credential shall be an A0 command and shall never be projected to MCP.
- **REQ-ENT.IAM-006** A service credential shall map to principal kind `service`, shall be scoped to named commands and sites, and shall stop working at once when revoked.
- **REQ-ENT.IAM-007** Every role shall declare which side-effect classes and sites it grants, and no role shall grant M1, S1 or A0 to an agent principal.
- **REQ-ENT.IAM-008** The server shall record each sign-in, refusal, role change and credential change in the Access Ledger with the identity pseudonymised per tenant.
- **REQ-ENT.IAM-009** The mapping from server sessions to principal kinds `human`, `script` and `agent` shall follow D-030 once decided, and until then every non-service server session shall map to `script` with the user as `on_behalf_of`.

## Interfaces

- **Offers:** `AuthorizedCaller { principal, roles, sites }` to `ENT.SRV`, `ENT.SYNC`, `ENT.EVENTS`, `ENT.CONNECT`, `ENT.DASH`; `server.*_user`, `server.*_role`, `server.*_idp`, `server.*_directory`, `server.*_credential` commands; only R0 listing tools on MCP.
- **Consumes:** principal type and kinds (`CORE.BUS.PRIN`), Access Ledger format and pseudonymisation (`CORE.JRN.STREAM`), profile definitions (`TRUST.PROFILE.GATE`).

## Decomposition hints

OIDC, SAML, LDAP (directory sync), ROLES (roles, scopes, mapping), CRED (service credentials), PRIN (server channel and principal mapping; blocked on D-030), AUDIT (Access Ledger events).

## Open questions

- D-030: which principal kind a person signed in to the server gets, and whether W3 is possible from the web client.
- `CORE.BUS.PRIN` has a closed channel-kind enum with no server-session kind. If D-030 picks option A or C, that node must change; ENT may not edit it.
- Identity pseudonym registry is owned by TRUST; whether the server keeps its own pseudonym key per site or per organisation is unresolved.

## Children

_None yet: status `open`._
