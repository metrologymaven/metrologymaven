---
id: OUTR.COMM.CHAN
title: Community channel inventory and listing
level: 3
parent: OUTR.COMM
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.COMM-001, REQ-OUTR.COMM-002, REQ-OUTR.COMM-008]
covers: []
depends_on: [OUTR.BRAND.LINT]
uses_artifacts: [OUTR.COMM.MOD]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.COMM.CHAN: Community channel inventory and listing

## Purpose

The set of official community channels (forum, chat, issue tracker, mailing list), the criteria a channel must meet before it becomes official, and the public channel listing on the website, generated from one inventory file. Channel choice is where the project's privacy and openness rules meet third-party providers, so the rules are checked on the inventory rather than remembered.

## Scope

**In:**
- `community/CRITERIA.md`: selection criteria for official channels (public readability or archive, tracking, data export, self-hosting option), written in our own words
- `community/channels.toml`: the inventory
- The `chan-check` command that validates the inventory
- The `chan-render` command that writes the listing page content for the website to import
- The ownership rule that makes the inventory change only through a reviewed pull request

**Out** (and which node owns it instead):
- Moderation procedure, rules text and abuse-report routes (owned by `OUTR.COMM.MOD`; this node links them)
- Counting activity on the channels (owned by `OUTR.COMM.METRIC`)
- Vendor and university program contact routes (owned by `OUTR.PROG`)
- Site build and page template (owned by `OUTR.SITE.GEN` and `OUTR.SITE.PAGES`)

## Requirements

- **REQ-OUTR.COMM.CHAN-001** The project shall keep one machine-readable channel inventory file, and the website's channel listing shall be generated from it by `chan-render`.
- **REQ-OUTR.COMM.CHAN-002** Each inventory entry shall state the channel's ID, kind, purpose, provider, URL, moderation-rules link, abuse-report route, `readable_without_account`, `public_archive` (URL or none) and `provider_tracking` (`none`, `can_disable` or `cannot_disable`), and `chan-check` shall fail naming every missing field.
- **REQ-OUTR.COMM.CHAN-003** If an entry is neither readable without an account nor linked to a public read-only archive, then `chan-check` shall fail with `not_publicly_readable` and name the channel.
- **REQ-OUTR.COMM.CHAN-004** If an entry records `provider_tracking = "cannot_disable"`, then `chan-check` shall fail with `no_tracker_free_alternative` unless the entry names, in `alternative`, another inventory channel whose tracking is `none`.
- **REQ-OUTR.COMM.CHAN-005** The listing shall show, for each channel, its purpose, URL, moderation-rules link, how to report abuse, and a plain note when the provider tracks members, with the tracker-free alternative named.
- **REQ-OUTR.COMM.CHAN-006** The repository's code-owners file shall assign the inventory file to the maintainers group, so that a channel becomes official only through a pull request approved by a maintainer other than the author (D-003), and a test shall fail if that assignment is missing.
- **REQ-OUTR.COMM.CHAN-007** The rendered listing shall pass the outreach claim lint with zero errors.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/comm/chan/ok.toml` (forum readable without account, chat with a public archive, issue tracker, mailing list with archive), when `fid-outreach chan-check ok.toml --json` runs, then it exits 0.
- **AC-2** (error path) Given `closed.toml` (a chat requiring an account with `public_archive = "none"`), when the check runs, then it exits 1 with `not_publicly_readable chat`.
- **AC-3** (error path) Given `tracking.toml` (a channel with `provider_tracking = "cannot_disable"` and no `alternative`), when the check runs, then it exits 1 with `no_tracker_free_alternative`; with `alternative` naming a channel whose tracking is `none`, it exits 0.
- **AC-4** (error path) Given `missing-field.toml` (no abuse-report route), when the check runs, then it exits 1 with `field_missing abuse_report`.
- **AC-5** Given `ok.toml`, when `fid-outreach chan-render ok.toml --out channels.md` runs, then `channels.md` lists all four channels with purpose, URL, moderation-rules link and abuse-report route, and the claim lint exits 0 on it.
- **AC-6** Given the outreach repository's code-owners file, when `tests/chan_owners.rs` runs, then `community/channels.toml` is assigned to the maintainers group.
- **AC-7** (determinism) Given the same inventory, when `chan-render` runs twice, then the outputs are byte-identical.

## Design

- **Commands:** `fid-outreach chan-check <inventory> [--json]` and `fid-outreach chan-render <inventory> --out <file>`. Project tooling; nearest class R0.
- **Inventory entry:** `[[channel]] id, kind (forum | chat | tracker | mailing_list | other), purpose, provider, url, moderation_rules, abuse_report, readable_without_account, public_archive, provider_tracking, alternative?, self_hosted, data_export`.
- **Listing output:** Markdown with front matter for OUTR.SITE.GEN; channels in inventory order; a tracking note in plain words.
- **Errors:** `field_missing`, `not_publicly_readable`, `no_tracker_free_alternative`, `alternative_unknown`, `duplicate_id`, `inventory_parse`; exit 0, 1 findings, 2 unreadable input.
- **Determinism:** D0.
- **Crate:** `fid-outreach`, module `channels`; data and criteria in the outreach repository under `community/`.

## Test fixtures

Hand-written inventories under `fixtures/outr/comm/chan/` (`ok`, `closed`, `tracking`, `missing-field`) using `example.org` URLs, CC-BY-4.0.

## Build prompt

> In the outreach repository, write `community/CRITERIA.md` (in your own words: an official channel must be readable without an account or have a public read-only archive; tracking must be off or, if it cannot be disabled, a tracker-free alternative must exist; data export and self-hosting are preferred) and create `community/channels.toml` (initially empty or with the channels maintainers have already agreed). Assign `community/channels.toml` to the maintainers group in the code-owners file.
>
> In the Rust crate `fid-outreach`, add module `channels` with `fid-outreach chan-check <inventory> [--json]` and `fid-outreach chan-render <inventory> --out <file>`. Inventory entries `[[channel]]` have `id`, `kind`, `purpose`, `provider`, `url`, `moderation_rules`, `abuse_report`, `readable_without_account`, `public_archive` (URL or `none`), `provider_tracking` (`none`, `can_disable`, `cannot_disable`), optional `alternative`, `self_hosted`, `data_export`. Check: `field_missing <field>`, `duplicate_id`, `not_publicly_readable <id>` (no account-free reading and no archive), `no_tracker_free_alternative` (cannot-disable tracking without an `alternative` whose tracking is `none`), `alternative_unknown`. Render Markdown with front matter listing purpose, URL, moderation-rules link, abuse-report route and a tracking note per channel; run the existing `claim_lint` on the result.
>
> Tests with fixtures under `fixtures/outr/comm/chan/` for the cases above, the code-owners assignment, and byte-identical rendering.
>
> Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; no network (never probe the channel URLs); BTreeMap rather than HashMap; allowlisted dependency licences only.

## Open questions

- Which hosted providers meet REQ-003 and REQ-004 is not yet researched [unverified] (carried from OUTR.COMM); maintainers fill the inventory using the criteria.
- The former code dependency on OUTR.SITE.PAGES is replaced: this node writes a page file that OUTR.SITE.GEN imports (OUTR.SITE.PAGES lists the site's other pages). Whether the channel listing is imported by OUTR.SITE.GEN or OUTR.SITE.PAGES is for the SITE nodes to settle.
- Branch protection (approval by a maintainer other than the author) is configured by ECO.REPO on the repository host; this node only checks the code-owners assignment.

## Children

_Leaf: none._
