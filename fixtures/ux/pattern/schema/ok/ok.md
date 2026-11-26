```toml
format = "pattern-spec@1"
id = "pat.command-run"
title = "Run a command from the palette"
trigger = "The person presses Ctrl+K or chooses Run command from the menu."
states = ["closed", "filtering", "running", "done"]
commands = ["view.select_*@1", "palette.run_command@1"]
cancel = ["Escape closes the palette; no command runs and the selection is unchanged."]
failure = ["A refused command shows its error code and one suggested fix inside the palette."]

[[steps]]
n = 1
actor = "human"
components = ["comp.field"]
input = "Text that names the command."

[[steps]]
n = 2
actor = "human"
components = ["comp.field", "comp.button"]
input = "The choice of one matching command."

[[steps]]
n = 3
actor = "system"
components = []
input = "None; the command bus runs the chosen command."

[[keyboard]]
step = 1
keys = "Ctrl+K, then type"

[[keyboard]]
step = 2
keys = "Up and Down, then Enter"
note = "Enter on a command that needs parameters opens its form."

[[journeys]]
map = "fx-run"
steps = [2, 3]

[[journeys]]
map = "fx-review"
steps = [1]
```

# Run a command from the palette (synthetic fixture)

The palette lists the commands that match the typed text; choosing one runs it once.
