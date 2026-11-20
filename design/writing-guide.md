# UI writing guide

Spec node: `UX.SYSTEM.PRINC` (REQ-UX.SYSTEM.PRINC-003, -004; REQ-UX.SYSTEM-010). Licence: CC-BY-4.0.

This guide covers English source text in the user interface, CLI help and report labels. Message
IDs, catalogues and translation belong to I18N; token values belong to `UX.TOKENS`. Until D-002
names the product, UI copy uses the placeholder `{{FAMILY_NAME}}` and never a candidate name.

## Voice

- **Plain.** Short sentences, common words, active voice. Write "Select a datum", not "Datum
  selection is required to be performed".
- **Exact.** Say what happened, to what, with the numbers. Write "3 of 42 points are outside the
  probe's stylus radius check", not "Some points look wrong".
- **No blame.** Describe the state and the next step, not the user's mistake. Write "The file
  has no units. Choose mm or inch to continue.", not "You forgot to set units".
- **No hype and no humour** in working screens, errors and reports.
- **Errors** say what failed, why, and what to do next, and carry their typed error code
  (CONSTITUTION §3) so that the message can be searched.
- **Agent text** says that an agent wrote it. An agent never "approves", "signs" or "releases";
  it "proposes" or "suggests" (see Reserved words).

## Preferred terms

The UI uses one term per concept. The right-hand column describes, in our own words, how other
tools and shop-floor usage often refer to the same thing, so that users can map their habits;
those phrasings are descriptions, not product-specific names, and none is copied from another
product [unverified: usage varies between tools and sites].

| Concept | Use in the UI | Common other phrasings (described) |
|---|---|---|
| A geometric element fitted from points | feature | "element", "geometry item" |
| The values that define where a feature should be | nominal | "theoretical", "CAD value", "target" |
| The value computed from measured points | actual | "measured", "real value" |
| Actual minus nominal | deviation | "error", "delta", "difference" |
| The coordinate system built from datums | datum reference frame (short: frame) | "alignment", "base system", "part coordinate system" |
| The steps that build a frame | alignment | "setup", "registration", "base alignment" |
| A toleranced property being evaluated | characteristic | "dimension", "callout", "check" |
| A tolerance zone result within limits | within tolerance | "in tol", "good", "pass" |
| A tolerance zone result outside limits | out of tolerance | "out of tol", "bad", "fail", "reject" |
| A reusable, versioned sequence of measurement steps | program | "routine", "part program", "inspection plan" |
| One execution of a program on one part | run | "measurement", "cycle", "job" |
| The record of what happened, in order | journal | "log", "history", "audit trail" |
| A point-sampling device on the machine | probe | "sensor", "tip" |
| A stated uncertainty of a result | uncertainty (with coverage factor) | "accuracy", "error band", "plus/minus" |
| The rule that turns a value and its uncertainty into a decision | decision rule | "acceptance rule", "guard band setting" |
| A change an agent suggests for a person to accept or reject | proposal | "suggestion", "recommendation" |

When a standard defines the concept, cite its designator and clause in help text (for example
"ISO 1101 [unverified: clause]") and paraphrase; never paste standards text.

## Reserved words

Some words carry legal, quality-system or security meaning. Each has exactly one permitted
meaning and may appear only under the listed message-ID prefixes. The machine-readable list is
[`reserved-words.json`](reserved-words.json) (`reserved-words@1`); `fid-design words-check`
reports every UI string that uses a reserved word, or one of its listed inflections, outside its
prefixes. Matching is whole-word and case-insensitive.

| Word | Only permitted meaning | Message-ID prefixes |
|---|---|---|
| approve | A person's recorded approval of an agent proposal, a program or a run. Never an automatic or agent action. | `program.release.`, `review.human.`, `run.approve.` |
| conforms | A characteristic's result lies within its tolerance under the stated decision rule. | `gdt.result.`, `rpt.result.` |
| release | Publishing a reviewed program version, or a software version, for use. | `program.release.`, `rel.` |
| sign | Creating or applying a cryptographic signature. | `program.release.sign.`, `trust.sign.` |
| verified | A check against a stated reference passed and was recorded: a signature or digest check, or an instrument performance verification. | `cal.verify.`, `trust.verify.` |

Use these instead elsewhere:

- "Apply", "Accept" or "Use" for taking an agent proposal into a draft (the person's approval is
  a separate, recorded step under `review.human.`).
- "Log in" and "Log out" for authentication (not "sign in").
- "What's new" or "Changes" for update notices (not "release").
- "Checked" or "Passed" for an ordinary validation (not "verified").
- "Within tolerance" for a single value (not "conforms") outside result and report screens.

The message-ID prefixes follow the command-ID based scheme that I18N defines; they are
provisional until `I18N.CATALOG` fixes the scheme, and changing them is a reviewed change to
`reserved-words.json`.

## Numbers and units

- **Value and unit always together.** Every quantity shows its unit in the same cell or label:
  "12.004 mm", "0.5°", "20.0 °C". A column header may carry the unit only when every cell in the
  column has the same unit and the header stays visible while scrolling.
- **Show the formatter's output verbatim.** Numbers are formatted by the shared number formatter
  (`NUM.FMT`) from the value, its unit and the user's display precision. UI code never rounds,
  truncates, pads or re-formats that string, and never formats a number itself.
- **Never re-round.** A value shown in two places shows the same digits in both. Summaries and
  tooltips use the same formatter call as the table.
- **Signs.** Deviations always show their sign ("+0.012 mm", "−0.004 mm"), as the formatter
  produces it.
- **Uncertainty** is shown with its coverage factor: "U = 0.003 mm (k = 2)".
- **Ranges and limits** repeat the unit on both ends when they could be read separately:
  "10.000 mm to 10.020 mm".
- **Counts** are integers without units and with the noun: "42 points", "1 feature".
- **No unit conversion in copy.** If a value must be shown in a second unit, call the formatter
  for that unit too.

## Command titles

- Command titles (menu entries, palette entries, buttons and registry command titles) use
  **sentence case** and start with a verb: "Create datum reference frame", "Export report",
  "Run program".
- Proper nouns, standard designators and unit symbols keep their own case: "Import STEP file",
  "Check ISO 10360 probe test" [unverified: example designator usage].
- No trailing full stop. An ellipsis ("…") only when the command opens a dialog that asks for
  more input before it acts.
- Titles name the action, not the object state: "Accept proposal", not "Proposal accepted".
- Keep titles under 40 characters so that they fit the palette and the narrow toolbar.
