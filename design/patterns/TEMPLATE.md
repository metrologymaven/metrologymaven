```toml
# pattern-spec@1 template (spec node UX.PATTERN.SCHEMA). Licence: CC-BY-4.0.
# Copy this file to patterns/<name>.md, set id = "pat.<name>" and replace every value.
# `fid-design pattern-check --root design/patterns --package design --journeys
# acceptance/journeys/persona` checks this file like any other spec (add --manifest <registry
# manifest> to match the command patterns), so the references below are real: comp.template is
# the component template and per-01 is a shipped journey map.
# Top-level keys come before the first [[table]]: TOML puts a key written after a [[table]]
# header inside that table.
format = "pattern-spec@1"
id = "pat.template"
title = "Pattern spec template"

# What starts the pattern.
trigger = "Replace with what the person or the system does to start the pattern."

# The pattern's states, as lowercase names.
states = ["idle", "editing", "done"]

# The registry commands the pattern runs, as <name glob>@<major> patterns (`*` matches any run
# of command-name characters). commands = [] for a pattern that runs no command. With
# --manifest, a pattern that matches no command is the warning PAT-COMMAND-UNMATCHED.
commands = ["project.create_project@1"]

# The cancel path and the failure path, one line per step. Neither may be empty.
cancel = ["Replace with what Escape or the cancel control does, and what is kept or discarded."]
failure = ["Replace with what the person sees when the command is refused, and the suggested fix."]

# The steps, numbered 1, 2, 3, .. in order. actor is one of human, operator, admin, agent,
# system; components lists the comp.<name> specs of the package the step uses ([] for none).
[[steps]]
n = 1
actor = "human"
components = ["comp.template"]
input = "Replace with the input the step expects."

[[steps]]
n = 2
actor = "system"
components = []
input = "None; the command bus runs the command."

# One complete keyboard path: every step of a person (human, operator, admin) has an entry.
[[keyboard]]
step = 1
keys = "Tab to the control, then Enter"
note = "Optional: anything the keys alone do not say."

# The journey map steps the pattern serves (map_id and step numbers of a journey-map@1 file).
[[journeys]]
map = "per-01"
steps = [2]
```

# Pattern spec template

Replace this prose with the pattern's description: when it is used, what the person sees in
each state, and how it ends. The front block above is what `fid-design pattern-check` checks;
this prose is for people and is not checked.
