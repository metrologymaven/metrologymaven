---
id: OUTR.COMM.CHAN
title: Community channel inventory and listing
level: 3
parent: OUTR.COMM
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.COMM-001, REQ-OUTR.COMM-002, REQ-OUTR.COMM-008]
covers: []
depends_on: [OUTR.SITE.PAGES]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.COMM.CHAN: Community channel inventory and listing

## Purpose

The set of official community channels (forum, chat, issue tracker, mailing list), the criteria a channel must meet before it becomes official, and the public channel listing on the website. It is separate because channel choice is where the project's privacy and openness rules meet third-party providers.

## Scope

**In:**
- Selection criteria for official channels (public readability or archive, tracking, export of data, self-hosting option)
- The channel inventory file: each channel's purpose, provider, moderation-rules link, abuse-report route, readability and tracking status
- The channel listing page rendered on the website from that file
- A tracker-free alternative for any channel whose provider tracks members

**Out** (and which node owns it instead):
- Moderation procedure and log (owned by `OUTR.COMM.MOD`)
- Counting activity on the channels (owned by `OUTR.COMM.METRIC`)
- Vendor and university program contact routes (owned by `OUTR.PROG`)
- Site build and page template (owned by `OUTR.SITE.GEN` and `OUTR.SITE.PAGES`)

## Requirements

- **REQ-OUTR.COMM.CHAN-001** The project shall keep one machine-readable channel inventory file, and the website's channel listing shall be generated from it.
- **REQ-OUTR.COMM.CHAN-002** Each inventory entry shall state the channel's purpose, provider, URL, moderation-rules link, abuse-report route, whether it is readable without an account, and whether the provider tracks members.
- **REQ-OUTR.COMM.CHAN-003** If an inventory entry is neither readable without an account nor linked to a public read-only archive, then the inventory check shall fail and name the channel.
- **REQ-OUTR.COMM.CHAN-004** If an entry records provider tracking that cannot be disabled, then the inventory check shall fail unless the entry names a tracker-free alternative channel that is itself in the inventory.
- **REQ-OUTR.COMM.CHAN-005** A channel shall become official only through a pull request that adds its inventory entry and is approved by a maintainer other than the author, following D-003.

## Interfaces

- **Offers:** the channel inventory file and its check; the channel listing page content.
- **Consumes:** OUTR.SITE.PAGES (rendering); GOVERNANCE.md code of conduct (link); OUTR.COMM.MOD (moderation-rules and abuse-report links).

## Decomposition hints

CRIT (selection criteria and provider survey), INV (inventory file and check), PAGE (listing page).

## Open questions

- Which hosted providers meet REQ-OUTR.COMM.CHAN-003 and -004 is not yet researched [unverified] (carried from OUTR.COMM).

## Children

_None yet: status `open`._
