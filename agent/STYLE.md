# Content standard for task guides, Skills and recipes

Spec node: `AGENT.SKILLS.STYLE`. Checked by `cargo run -p fid-agent --bin agent-style-lint`. The linter is not yet run by CI: the repository has no registry manifest or generated Skills to lint yet, so wiring it into the workflow is a follow-up (ECO.CI).

This standard covers the prose agents are taught from: the title, summary and step purposes of every curated task guide (the guides `agent/catalog.json` names), the rendered Skill of each curated guide, and the title, prompt and commentary of each curated recipe. All examples below are written by the project for this page. This page itself is not linted, because it shows the non-compliant examples on purpose.

The linter reports `file:line: rule: detail`. Prose read from a JSON field is reported as `<file>#<pointer>`, and its line counts lines within that field.

## Rule 1: no numbers dressed as results (`result_like_number`)

Prose never states a measured or computed value. Values of record live in result records, with their unit, uncertainty and provenance. In prose, a number followed by a length unit (such as nm, µm, mm, m, inch), an angle unit (such as deg, °, rad, arcmin) or `%` is flagged, unless it sits inside a fenced command example. Step numbers, version numbers and counts without a unit are fine, and so are temperatures (`°C`, `°F`, `degrees Celsius`) and "degrees of freedom".

- Compliant: "Fit a cylinder to the scan and read its diameter from the result record."
- Compliant: a fenced command example such as `fid demo fit-cylinder --max-gap 0.2 mm`.
- Non-compliant: "The bore measures 12.004 mm, so it passes."

This is a heuristic [unverified]. A maintainer can allow one finding by adding `<rule> <line> <file>` to the allow list (`--allow`), reviewed in the pull request like any other change.

## Rule 2: never instruct a forbidden class (`forbidden_instruction`)

Agents never perform M1 (motion), S1 (signing), A0 (admin and safety configuration) or W3 (controlled record release) commands (CONSTITUTION §3). Prose must not name such a command, by command ID, by ID with major, or by MCP tool name, anywhere, including fenced examples. The one exception: the first such name after the phrase "request for a human" on the same line, because filing a request is what an agent may do. Any further name on that line is flagged.

- Compliant: "File a request for a human to run `journal.promote_protected_branch@1` after review."
- Non-compliant: "Now call `journal_promote_protected_branch` to finish."

## Rule 3: end state-changing guides with a proposal (`missing_proposal_step`)

A curated guide with any W1 or W2 step changes document state, so an agent's work goes to a person for adoption. Its last two steps must be `agent.attach_brief@1` and then `journal.request_promote_branch@1`.

- Compliant: scan, save the drafted plan, attach the proposal brief, request promotion of the agent branch.
- Non-compliant: scan, save the drafted plan (and stop).

## Rule 4: keep Skills short (`over_budget`)

The rendered body of a curated Skill (everything after its front matter, counted as whitespace-separated words) stays within 1,500 words [unverified budget]. Longer material goes into recipes that the agent fetches on demand.

- Compliant: a Skill that lists the steps with one sentence each and links its recipes.
- Non-compliant: a Skill that pastes a full worked example of 1,600 words into its body.

## Rule 5: no standards text (`possible_standards_text`)

Standards text (ISO, ASME, VDA and others) is copyrighted and is never stored or served (CONSTITUTION §5). Cite designators and clause numbers only, and explain in the project's own words. The linter flags two signals [unverified heuristic]:

- a run of 12 or more consecutive prose words that match one entry of `agent/style-denylist.txt` word by word (each prose word starts with the entry's stem);
- a quotation of more than 25 words, in double quotes or as a Markdown blockquote.

The deny list holds project-written phrase stems only, never standards text.

- Compliant: "Qualify the probe as the plan's referenced standard requires (cite the designator and clause)."
- Non-compliant: a blockquote that copies a paragraph of a standard.

## When the linter is wrong

The number and standards-text rules are heuristics and will sometimes flag fine prose. Rephrase first. If rephrasing would make the text worse, add an allow-list line and say why in the pull request; a maintainer reviews it.
