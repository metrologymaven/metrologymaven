---
id: AUTO.CLI.DISPATCH
title: Registry command dispatch onto the bus
level: 3
parent: AUTO.CLI
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.CLI-001, REQ-AUTO.CLI-002, REQ-AUTO.CLI-005]
covers: []
depends_on: [AUTO.CLI.TREE, AUTO.CLI.ARGS, AUTO.CLI.GATE, AUTO.CLI.OUT, AUTO.CLI.JOB, AUTO.CLI.TARGET.SELECT, AUTO.SURF.JOBS, AUTO.SURF.ROOTS, CORE.BUS.PRIN]
uses_artifacts: [CORE.BUS.PIPE.ERR]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-24
---

# AUTO.CLI.DISPATCH: Registry command dispatch onto the bus

## Purpose

The step between "the tree resolved `fid <domain> <verb-object>` to a command major" and "the envelope is printed". DISPATCH decides whether the caller may see that major at all, maps the remaining arguments onto the major's parameter schema, sends exactly one invocation through the target transport on a `cli` channel, and hands the final envelope to `AUTO.CLI.OUT`. It replaces the placeholder refusal `fid` prints today for every resolved registry command, so the CLI forms that other leaves defer to it (`fid migrate list-sources`, `fid migrate generate-coverage`, `fid report add-balloon` and the rest) run through the same bus as every other surface.

## Scope

**In:**
- Visibility of a resolved major for the session's principal: lifecycle `hidden` and `removed` majors, and root-configuration commands for every principal other than `human`, are treated as absent
- Refusing an absent major exactly as a path or major the tree does not hold (D-035), including refusals the bus returns as `not_found` or `hidden_command`
- The flag table of each major: one flag per top-level property of its params schema, reserved flags, collisions
- Splitting the argv after the command path into reserved flags and parameter flags; unknown, repeated and missing required flags
- Assembling the parameter document from the flags and the `--params` document
- The channel the invocation is sent on and the identity it carries
- The order of the pre-send steps (visibility, gate, arguments, raw-path gate, required check)
- Sending one invocation through the `AUTO.SURF.JOBS` tracker over the `TargetTransport` that `AUTO.CLI.TARGET.SELECT` chose, and passing the final envelope to `AUTO.CLI.OUT`

**Out:**
- Command paths, majors, `--major` and help text (`AUTO.CLI.TREE`)
- Parsing one value token into a typed value, quantities and reading the `--params` document (`AUTO.CLI.ARGS`)
- Dry-run handling, M1, S1 and A0 refusals and the raw-path gate (`AUTO.CLI.GATE`, `AUTO.SURF.ROOTS`)
- Envelope rendering and the exit-code table (`AUTO.CLI.OUT`)
- Progress display and interrupt-to-cancel (`AUTO.CLI.JOB`)
- Target choice, project open and lock, the `--confirm-human` upgrade and the transports themselves (`AUTO.CLI.TARGET.SELECT`, `AUTO.CLI.TARGET.HEADLESS`, `AUTO.CLI.TARGET.APP`)
- Principal derivation and the bus's own resolution and validation (`CORE.BUS.PRIN`, `CORE.BUS.PIPE.VALID`)
- Translating bus errors into envelope error codes (the transport behind `AUTO.SURF.JOBS`)

## Requirements

- **REQ-AUTO.CLI.DISPATCH-001** When the tree resolves an argv to a visible command major and every pre-send step passes, the CLI shall send exactly one invocation of that command ID and major through the `AUTO.SURF.JOBS` tracker over the `TargetTransport` chosen by `AUTO.CLI.TARGET.SELECT`.
- **REQ-AUTO.CLI.DISPATCH-002** The CLI shall derive one flag per top-level property of the major's params schema, `--` followed by the property name with every underscore replaced by a hyphen, and shall accept it as `--<flag> <value>` or `--<flag>=<value>`.
- **REQ-AUTO.CLI.DISPATCH-003** If a derived flag equals a reserved flag (`--json`, `--dry-run`, `--major`, `--help`, `--params`, `--project`, `--app`, `--confirm-human`, `--version`) or the derived flag of another property of the same major, then building the flag table shall fail with `flag_collision` naming the command major and the property.
- **REQ-AUTO.CLI.DISPATCH-004** If an argument after the command path is neither a reserved flag nor a flag of the major's table, or repeats the flag of a property whose schema type is not `array`, then the CLI shall exit 2 with `usage_error` naming that argument, suggest the nearest flag of the same major within edit distance 2, and send nothing.
- **REQ-AUTO.CLI.DISPATCH-005** If a property listed in the schema's `required` array is given neither as a flag nor in the `--params` document, then the CLI shall exit 2 with `usage_error` naming its flag and send nothing.
- **REQ-AUTO.CLI.DISPATCH-006** The CLI shall build the parameter document as one RFC 8785 JSON object keyed by schema property names, holding only the values given by flags and the `--params` document, without inserting schema defaults.
- **REQ-AUTO.CLI.DISPATCH-007** When a property's schema type is `object` and it is not a quantity, the CLI shall take its flag value as one JSON object read with the size and depth limits of REQ-AUTO.CLI.ARGS-004.
- **REQ-AUTO.CLI.DISPATCH-008** When the target is headless or no target is selected, the CLI shall send the invocation on a channel of kind `cli` whose authenticated identity is the operating-system user running the process and whose session is the process's session, so the bus derives the principal as REQ-CORE.BUS.PRIN-003 requires.
- **REQ-AUTO.CLI.DISPATCH-009** The CLI shall put no principal, actor, on-behalf-of or session field into the parameter document or the invocation, on any target (REQ-CORE.BUS.PRIN-002).
- **REQ-AUTO.CLI.DISPATCH-010** The CLI shall treat as absent every major whose lifecycle state is `hidden` or `removed`, and, while the session's principal kind is not `human`, every command for which `is_root_config` is true (REQ-AUTO.SURF.ROOTS-006).
- **REQ-AUTO.CLI.DISPATCH-011** The CLI shall resolve every argv as against a tree that does not hold the majors absent under REQ-010: a path without `--major` resolves to its newest visible major, and a path or major with no visible match produces the same stdout bytes, stderr bytes and exit code as such a tree, with suggestions and listed majors drawn only from visible ones (D-035, extended as stated under Design).
- **REQ-AUTO.CLI.DISPATCH-012** If the final envelope fails with an error that the bus raised as `hidden_command`, or as `not_found` with reason `unknown_command`, `unknown_major` or `removed_major` (REQ-CORE.BUS.PIPE.ERR-001), then the CLI shall discard that envelope and produce the REQ-011 refusal for the invoked argv; every other error, including `not_found` with any other reason, shall pass to `AUTO.CLI.OUT` unchanged.
- **REQ-AUTO.CLI.DISPATCH-013** The CLI shall run the pre-send steps in the order visibility (REQ-010), the `AUTO.CLI.GATE` class and dry-run checks, flag mapping with `AUTO.CLI.ARGS` parsing, the `AUTO.CLI.GATE` raw-path check, and the required check (REQ-005), and shall send nothing to the bus when any of them refuses.
- **REQ-AUTO.CLI.DISPATCH-014** When the final envelope arrives, the CLI shall pass it unchanged to `AUTO.CLI.OUT` and exit with the code REQ-AUTO.CLI.OUT-002 assigns to it.

## Acceptance criteria

- **AC-1** Given fixture manifest `fixtures/manifest/cli-dispatch.json` and the `FakeBus` of `fid-surface` behind a test `TargetTransport`, when `fid demo set-offset --offset 12.700mm --label=a --json` runs, then the fake bus receives exactly one invocation of `demo.set_offset@1` with parameter document `{"label":"a","offset":{"unit":"mm","value":"12.700"}}` (RFC 8785 bytes), on a channel of kind `cli` with the test OS user as identity, and with no principal, actor, on-behalf-of or session field; stdout is exactly the one envelope the fake bus returned, and the exit code is 0.
- **AC-2** Given the scripts of `fixtures/cli/dispatch/outcomes.json` (one per outcome and per error code of `AUTO.SURF.ENV`, including `policy_refused`, `validation_error`, `cancelled`, `not_run`, `no_reply` and `request_filed`), when `fid demo show-thing --json` runs against each, then stdout is the scripted envelope unchanged and the exit code equals the REQ-AUTO.CLI.OUT-002 code for it.
- **AC-3** (error path, D-035) Given principal kind `script` and, in the fixture, `demo.old_cmd@1` (lifecycle `hidden`), `demo.mix@2` (lifecycle `hidden`, with `demo.mix@1` active) and root-configuration command `project.set_import_root@1`, when `fid demo old-cmd`, `fid demo mix --major 2`, `fid project set-import-root` and `fid demo old-cmdd` run, each with and without `--json`, then stdout, stderr and exit code are byte-identical to those of the same argv against a tree built from the fixture with those three majors deleted; no output names `old_cmd`, major 2 of `demo.mix` or `set_import_root` unless the argv typed it, and the fake bus receives nothing. Given the same principal, when `fid demo mix --json` runs with no `--major`, then the fake bus receives exactly one invocation, of `demo.mix@1`.
- **AC-4** (error path, D-035) Given a visible `demo.show_thing@1` and a fake bus that answers it in turn with `hidden_command/hidden_major`, `not_found/unknown_command`, `not_found/unknown_major` and `not_found/removed_major`, when `fid demo show-thing --json` runs against each, then every output and exit code equals those of `fid demo show-thing` against a tree that does not hold `demo.show_thing@1`.
- **AC-10** (error path) Given a fake bus that answers `fid demo show-thing --json` in turn with `not_found/unknown_entity`, `not_found/unknown_branch`, `not_found/unknown_job` and `validation/schema`, then stdout is each scripted envelope unchanged, with its own code, reason and detail, and the exit code is the REQ-AUTO.CLI.OUT-002 code for it.
- **AC-11** Given `demo.set_offset@1` whose schema gives `label` the default `"none"`, when `fid demo set-offset --offset 1mm` runs, then the parameter document the fake bus receives is exactly `{"offset":{"unit":"mm","value":"1"}}`, with no `label` key.
- **AC-12** (error path) Given `--params -` with stdin `{"label":"a"}` and no `--offset` flag, when `fid demo set-offset --params -` runs, then exit 2 with `usage_error` naming the missing `--offset` and the fake bus receives nothing; given stdin `{"offset":{"value":"2","unit":"mm"}}` and no flag, then the invocation is sent with that offset.
- **AC-13** (error path, ordering) Given principal kind `script`, when `fid demo old-cmd --no-such-flag` and `fid demo mix --major 2 --dry-run --ofset x` run, then each output and exit code equals the absent-major refusal of `fid demo old-cmd` and of `fid demo mix --major 2` without the extra flags (that is, of a tree without the hidden majors), no output names `--no-such-flag`, `--ofset` or `dry_run_unsupported`, and the fake bus receives nothing.
- **AC-5** (error path) Given `fid demo set-offset --ofset 1mm`, then exit 2 with `usage_error` naming `--ofset` and suggesting `--offset`; given `fid demo set-offset --label a`, then exit 2 naming the missing `--offset`; given `fid demo set-offset --offset 1mm --label a --label b`, then exit 2 naming `--label`; given `fid demo set-offset --offset 1mm stray`, then exit 2 naming `stray`; in every case the fake bus receives nothing.
- **AC-6** (error path) Given fixture `fixtures/manifest/cli-dispatch-collision.json`, where `demo.bad_flags@1` has a property `dry_run` and `demo.twin_flags@1` has properties `a_b` and `a-b`, when the flag table is built, then it fails with `flag_collision` naming each command major and property; and given the workspace registry manifest, the flag table builds with no collision.
- **AC-7** Given `fixtures/manifest/min-classes.json`, when every visible major is invoked with the smallest argv that satisfies its required properties, then each one not of class M1, S1 or A0 reaches the fake bus exactly once, no M1, S1 or A0 major reaches it, and no output contains the text `AUTO.CLI.DISPATCH`.
- **AC-8** Given principal kind `human` (a test `TargetTransport` that reports an upgraded session; see Open questions on reaching `HumanPresence`), when `fid project set-import-root --json` runs against `fixtures/manifest/roots-config.json` (its params schema has no properties), then the fake bus receives one invocation of `project.set_import_root@1`.
- **AC-9** Given `fid demo place-label --anchor '{"x":"1","y":"2"}'`, then the parameter document holds `anchor` as that object; given an `--anchor` value of 2 MiB, then exit 2 with `params_too_large` and the fake bus receives nothing.

## Design

- **Registry command(s):** none of its own; dispatches every registered command major the tree holds.
- **Data structures:** `FlagTable { major: CommandId, flags: BTreeMap<String, FlagSpec> }` with `FlagSpec { property, schema_type, required, quantity: bool }`, built once per major from the embedded tree; `Visibility { principal_kind }` with `fn visible(&CliMajor) -> bool`; `Invocation { command_id, major, params: CanonicalJson, dry_run: bool }` handed to the tracker. The embedded tree entry gains `lifecycle` and `root_config` fields (from the manifest's lifecycle record and `is_root_config`), so visibility needs no bus call.
- **Algorithm:**
  1. `AUTO.CLI.TREE` resolves the path and major; the session's principal kind is the one held after any `--confirm-human` upgrade (`script` otherwise).
  2. Visibility: drop lifecycle `hidden` and `removed` majors, and root-configuration commands unless the principal kind is `human`; then re-run the tree's own resolution over the filtered view, so a path without `--major` takes its newest visible major and a miss (unknown path, unknown major, nearest match) is built by the same function as a real miss.
  3. `AUTO.CLI.GATE` class and dry-run checks.
  4. Split the rest of the argv into reserved flags and table flags; `--flag=value` and `--flag value`; an array property appends one element per occurrence in argv order; an object property takes one bounded JSON value; each value goes to the `AUTO.CLI.ARGS` parser; `--params` is merged by ARGS with explicit flags winning.
  5. `AUTO.CLI.GATE` raw-path check, then the required check.
  6. Send through the tracker; `AUTO.CLI.JOB` shows progress and handles interrupts; the final envelope goes to `AUTO.CLI.OUT`, except that a failure the bus raised as `hidden_command`, or as `not_found` with reason `unknown_command`, `unknown_major` or `removed_major`, is replaced by step 2's refusal. Handler and lookup errors such as `not_found/unknown_entity` and `not_found/unknown_branch` pass through unchanged.
- **D-035 scope:** D-035 and REQ-CORE.BUS.PIPE.VALID-009 make `never`-projected commands indistinguishable from unknown ones for `agent` callers. Applying the same rule to lifecycle-hidden majors for every CLI caller extends that decision. The reason: for a `script` caller the bus answers a hidden major with `hidden_command` (REQ-CORE.BUS.PIPE.VALID-002), which confirms that the major exists, while a hidden major is meant to be usable only by replay. One refusal path also keeps the CLI's miss handling in one function. Root-configuration commands follow REQ-AUTO.SURF.ROOTS-006 directly.
- **Not a security control:** the visibility filter is a convenience that keeps CLI output uniform. The manifest is public, and a caller can reach the bus without `fid`. Enforcement stays with the bus (REQ-CORE.BUS.PIPE.VALID-002 and -009, `CORE.BUS.POL`), and DISPATCH never relaxes a bus refusal.
- **Channel and principal:** the headless and no-project transports open a `cli` channel whose identity subject is the OS user and whose session key lives for the process; the bus derives `script`, or `human` when the session holds an upgrade grant (`CORE.BUS.PRIN`). The app target sends over the control channel, whose server binds the principal (`AUTO.CTRL.EXEC`); DISPATCH sends no principal data on either.
- **Errors:** `usage_error` (exit 2) for unknown, repeated, stray and missing flags and for absent majors; `flag_collision` when the flag table is built (a build-time test over the workspace manifest, so it never reaches a user); `params_too_large` and `params_parse` from ARGS (exit 2); everything after sending comes from the envelope.
- **Determinism:** N, because the tracker's idempotency key and the transport are not deterministic. The flag table, the parameter document and every pre-send refusal are D0: a pure function of the embedded tree, the argv and the principal kind.
- **Performance budget:** < 2 ms from argv to the invocation handed to the tracker for a 5,000-command tree, excluding command execution [unverified until measured].
- **Crate:** `fid-cli` (module `dispatch`). It links only `fid-surface`, `fid-types`, `fid-canon` and `fid-ctrl` (already linked for `AUTO.CTRL.CLIENT`), and reaches the bus only through `BusTransport`, so it adds no `surface_dep` edge (REQ-CORE.BUS.LINT-004).

## Test fixtures

`fixtures/manifest/cli-dispatch.json` (project-authored, CC-BY-4.0): `demo.show_thing@1` (R0), `demo.set_offset@1` (W2; required quantity `offset`, optional string `label` with schema default `"none"`), `demo.place_label@1` (W2; object `anchor`), `demo.old_cmd@1` (lifecycle `hidden`), `demo.mix@1` (active) and `demo.mix@2` (lifecycle `hidden`), and root-configuration command `project.set_import_root@1`. `fixtures/manifest/cli-dispatch-collision.json` for AC-6. `fixtures/cli/dispatch/outcomes.json`: scripted fake-bus answers for AC-2 and AC-4. All generated by `fixtures/manifest/make_fixtures.py`. Reuses `fixtures/manifest/min-classes.json`, `fixtures/manifest/roots-config.json`, the `FakeBus` of `fid-surface` tests and a fake `HumanPresence` provider.

## Build prompt

> In crate `fid-cli`, add module `dispatch` and replace the placeholder refusal that `fid` prints for every resolved registry command. (1) Visibility: extend the embedded `cli-tree.json` entry with `lifecycle` and `root_config`; for the session's principal kind (`script` unless a `--confirm-human` upgrade succeeded), treat lifecycle `hidden` and `removed` majors, and root-configuration commands for every kind but `human`, as absent, and resolve over the filtered view by re-running the tree's own resolution, so a path without `--major` takes its newest visible major and a miss gives the same stdout, stderr and exit code as a tree without them; suggestions and `--major` listings come only from visible majors (D-035). (2) Flag table per major: `--` plus the top-level params-schema property name with `_` replaced by `-`; `--f v` and `--f=v`; reserved flags `--json --dry-run --major --help --params --project --app --confirm-human --version`; a collision with a reserved flag or another property is `flag_collision`, checked by a test over the workspace manifest. Unknown, stray or repeated (non-array) flags and missing required properties are exit 2 `usage_error` naming the flag, with a nearest-flag suggestion (edit distance ≤ 2); arrays append per occurrence; object properties take one JSON value (1 MiB, depth 32). Values are parsed by the `AUTO.CLI.ARGS` parser; no defaults are inserted; the document is RFC 8785 JSON keyed by property name. (3) Order: visibility, `AUTO.CLI.GATE` class and dry-run checks, flags and ARGS, GATE raw-path check, required check; any refusal sends nothing. (4) Send one invocation through the `fid_surface` tracker over the `TargetTransport` chosen by `resolve_target`; the headless and no-project transports use a `cli` channel with the OS user as identity; never put principal, actor, on-behalf-of or session fields in the parameters. (5) Hand the final envelope unchanged to the `AUTO.CLI.OUT` renderer and exit with its code, except that a failure the bus raised as `hidden_command`, or as `not_found` with reason `unknown_command`, `unknown_major` or `removed_major`, becomes the refusal of step 1; every other error, including `not_found/unknown_entity` and `not_found/unknown_branch`, passes through unchanged. The visibility filter is a convenience, not a security control: never relax a bus refusal. Acceptance: AC-1..AC-13 of spec node AUTO.CLI.DISPATCH with `fixtures/manifest/cli-dispatch.json`, `cli-dispatch-collision.json`, `fixtures/cli/dispatch/outcomes.json`, `min-classes.json`, `roots-config.json` and the `fid-surface` `FakeBus`. Constraints: CONSTITUTION §3 (principals, registry visibility) and §8 (no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`); `BTreeMap` only; link no CORE crate other than the bus API (REQ-CORE.BUS.LINT-004).

## Open questions

- **Partial build (not `built_in`).** `fid-cli` module `dispatch` implements visibility and the re-resolution over the filtered tree (REQ-010, -011), the flag table and `flag_collision` (REQ-002, -003), argv splitting with unknown, repeated, stray and missing flags (REQ-004, -005), object values with the bounded reader (REQ-007), the parameter document without defaults (REQ-006), the pre-send order (REQ-013), the `cli` channel check (REQ-008), no identity fields (REQ-009), one invocation through the `fid_surface` tracker (REQ-001), the replacement of `hidden_command` and `not_found/unknown_command|unknown_major|removed_major` answers (REQ-012) and the hand-off to `AUTO.CLI.OUT` (REQ-014). The unbuilt dependencies are trait seams in that module: `ArgParser` (`AUTO.CLI.ARGS`), `Gate` (`AUTO.CLI.GATE`), `TargetTransport` (`AUTO.CLI.TARGET.SELECT`, with the session's principal kind and channel) and `ProgressSubscriber` (`AUTO.CLI.JOB`). The shipped `fid` binary applies visibility as `script` and still refuses every visible command with `precondition_failed`, because no `TargetTransport` exists. Open acceptance criteria: **AC-1** and **AC-12** pass with a test double of the ARGS parser (quantity parsing and the `--params` reader are `AUTO.CLI.ARGS`'s); **AC-7** passes with a test double of the GATE class check; **AC-8** passes only with a test transport that reports a `human` session, since the real `--confirm-human` upgrade needs `HumanPresence` behind a bus API (first item below). Covered against the real code: AC-2, AC-3, AC-4, AC-5, AC-6 (including the workspace manifest), AC-9, AC-10, AC-11, AC-13. Mark the leaf built once ARGS, GATE and TARGET.SELECT are built and plugged into the seams, and AC-8 runs against a real upgrade.
- The tracker's `BusRequest` has no mode field, so a dry-run invocation cannot be sent as one. Until it has one, `AUTO.CLI.GATE` must refuse every `--dry-run`, which it does today because the projection declares no dry-run support for any command.
- The bus code and reason that REQ-012 keys on are read from the envelope's `error.details` members `bus_code` and `bus_reason`, the names `fid_surface::roots::RootError::to_envelope` already uses. The transport translation (third item below) should fix those names for every surface.
- The real in-process transport needs every domain's command handlers, but REQ-CORE.BUS.LINT-004 lets `fid-cli` link no CORE crate other than the bus API crate (`fid-bus-api` in `lint-allowlist.toml`), and no node yet specifies that crate or the composition root that collects handlers. DISPATCH reaches the bus only through `BusTransport` and is tested against `FakeBus`. The end-to-end CLI forms that other leaves defer here also need `AUTO.CLI.TARGET.HEADLESS` built behind that API. The same rule makes `HumanPresence` and the upgrade grant of `CORE.BUS.PRIN` unreachable: they live in `fid-bus`, which `fid-cli` may not link, so AC-8 uses a test transport that reports an upgraded session, and a real `--confirm-human` upgrade needs the bus API to expose the upgrade. Cross-domain issue for `CORE.BUS`.
- REQ-AUTO.CLI.TARGET-005 runs project-free commands with no target, but `AUTO.CLI.TARGET.HEADLESS` specifies only the session over a project. DISPATCH uses whatever `TargetTransport` `resolve_target` yields; the no-project in-process session is for `AUTO.CLI.TARGET`.
- `AUTO.SURF.ENV`'s initial error-code set has no code for the bus's `hidden_command`, and no node owns the translation from bus errors to envelope codes for all surfaces. REQ-012 keys on the bus code that the transport carries in the envelope's error details. That translation should belong to `AUTO.SURF.ENV`, so that the CLI, MCP and Python cannot drift.
- `AUTO.CLI.TREE` help and `fid --help --json` are built from the manifest alone, so they still list lifecycle-hidden majors and root-configuration commands, while REQ-AUTO.SURF.ROOTS-006 says surfaces shall not list root-configuration commands for principals other than `human`. Filtering the listings is for `AUTO.CLI.TREE`; DISPATCH covers invocation only.
- `AUTO.CLI.OUT` replaces control characters in human output, so a multi-line text result (for example the Markdown of `fid migrate list-sources --format md`) cannot be compared with a file through human stdout. Under `--json` it is compared through the envelope's `result`. A raw-result output mode would be an `AUTO.CLI.OUT` change.

## Children

_Leaf: none._
