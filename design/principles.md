# Design principles

Spec node: `UX.SYSTEM.PRINC` (REQ-UX.SYSTEM.PRINC-001, -002). Licence: CC-BY-4.0.

Every design review cites the principles it checked by ID (`UX.SYSTEM.REVIEW`). Each principle
below has a stable ID in its heading, a short rationale, at least one "Do" and at least one
"Don't" example. The examples are written for this project; none is taken from another
product's screens, manuals or text. `fid-design princ-check` checks this structure.

Rules for editing this file: every second-level heading is a principle and reads
`## P-<ID>: <title>`; each principle has a `### Do` and a `### Don't` subsection with at least
one example. Adding, renaming or removing a principle ID is a change to the spec node first.

## P-DENSE: Dense but calm

Measurement work needs many values on screen at once, and operators compare them side by side.
Show a lot, but keep the screen quiet: aligned columns, one accent at a time, motion only when
something changed.

### Do

- Show a feature table with 40 rows at the default size, with right-aligned numbers in a
  monospaced figure style so decimal points line up.
- Use whitespace and thin rules to group columns instead of boxes and shadows.
- Animate only a state change (a run finished, a value went out of tolerance), and keep the
  animation under the duration token for short transitions.

### Don't

- Don't hide columns behind "show more" by default so that the first screen looks sparse.
- Don't give every panel its own header colour, icon and border; the eye then has no resting place.
- Don't use looping or decorative animation on a measurement screen.

## P-DATA: Data first

The measured data and the part are the subject; the application chrome is not. Space, contrast
and the first reading position go to results, deviations and the model.

### Do

- Give the 3D view and the results table most of the window; toolbars collapse to icons with
  text labels on hover and in the keyboard reference.
- Put the deviation and its tolerance next to each other in the first columns a reader meets.
- Let a user open any result at full width with one key.

### Don't

- Don't open a project on a dashboard of tiles that repeat what the results table already says.
- Don't place marketing, tips or news panels on working screens.
- Don't round a value to make a table narrower.

## P-EXPLAIN: Every number explainable

Any number the application shows can be traced to where it came from: which points, which
fit, which datum reference frame, which decision rule, which uncertainty statement. Trust in a
result depends on being able to check it.

### Do

- Make every result value open a "How was this computed" panel that lists the inputs, the
  algorithm and its version, the reference frame and the uncertainty contribution.
- Show the decision rule beside every pass or fail.
- Link a reported value to the journal entry that produced it.

### Don't

- Don't show a value whose source cannot be opened from the screen it is on.
- Don't merge results from different fits or frames in one column without saying so.
- Don't show "OK" without the rule and the numbers it was decided from.

## P-KEYS: Keyboard-complete

Every action can be reached and completed from the keyboard, in a predictable order, with a
visible focus. Many users work at a machine with a keyboard and no comfortable mouse, and
assistive technology depends on it.

### Do

- Give every command a place in the command palette, and the frequent ones a shortcut shown
  next to their menu entry.
- Keep a visible focus ring that meets the contrast token, on every focusable element.
- Let the 3D view be rotated, zoomed and stepped through features with keys.

### Don't

- Don't make an action reachable only by drag, right-click or hover.
- Don't trap focus inside a panel without a documented key to leave it.
- Don't reuse a common shortcut for a destructive action.

## P-FAMILIAR: Familiar workflows without incumbent trade dress

People who have used other metrology software should recognise the workflow: set up, align,
measure, evaluate, report. They should not recognise the look. We follow common task order and
standard terms, and we draw our own layouts, icons, colours and names (CONSTITUTION C9; trade
dress rules from `OUTR.BRAND.GUIDE`).

### Do

- Order a measurement job the way the task runs (setup, alignment, features, evaluation,
  report), because that order comes from the work itself.
- Use standard, neutral terms from the writing guide and from the cited standards by designator.
- Design icons, panel arrangement and colour roles from our own tokens and sketches, and record
  the references consulted in the originality declaration.

### Don't

- Don't copy another product's screen layout, icon set, colour scheme, dialog wording or
  product-specific names, even "for familiarity".
- Don't use screenshots of other products as design references in the repository.
- Don't name a feature after another vendor's trademarked term.

## P-NOCOLOR: Status never by colour alone

Pass, fail, warning and out-of-date states are always shown with a second cue: a word, a
symbol or a shape. Colour vision varies, printouts are often grey, and screens on the shop
floor are seen in bad light.

### Do

- Show a failed characteristic with the word "Out of tolerance", a distinct symbol and the
  failure colour together.
- Use patterns or markers, not only hue, to separate series in charts and in the deviation map.
- Check every status component in greyscale before review.

### Don't

- Don't mark a row as failed only by turning its background red.
- Don't rely on red and green as the only difference between two states.
- Don't use a colour legend as the only key to a deviation map; label the scale with values and units.
