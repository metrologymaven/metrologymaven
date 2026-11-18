# Reviewer checklist for app-crate PRs

Spec node: `ECO.APPS.POLICY` (REQ-ECO.APPS.POLICY-005). Use this list for any PR that touches a
crate in the `app` layer of `layers.toml`, or an app manifest.

`cargo xtask apps-policy` already catches the mechanical cases: an app crate that declares
registry commands (`POLICY-DEFN`), a shared crate that depends on an app crate or asks which
app is running (`POLICY-LEAK`), a command that differs between apps (`POLICY-IDENT`), and an
app crate with more than its entry point (`POLICY-SIZE`). This checklist covers what a tool
cannot decide. An app crate holds composition only: which manifest to load and how to start
the shared shell. Everything else lives in shared crates and is reached through the registry.

## Checks

- [ ] **No numeric logic hidden in UI code.** The entry point and any approved extra file
  contain no arithmetic on measured values: no averaging, fitting, filtering, tolerance or
  pass/fail decisions, unit conversion or rounding of results for display beyond the shared
  formatting helpers. Look for float arithmetic, loops over points, and constants such as
  thresholds or sigma factors. Such code belongs in a shared crate behind a registry command.
- [ ] **No app-specific branching through configuration values.** Shared crates do not read
  a setting, environment variable, feature flag or manifest field in order to behave
  differently per app (for example `if settings.mode == "inspect"`, or a default that only
  one app sets in order to switch an algorithm). Apps may choose which commands, panels,
  defaults and templates they enable; they may not change what a command does.
- [ ] **No copied domain code.** The PR does not copy or lightly adapt code from a shared
  crate (fitting, evaluation, file formats, report generation) into an app crate or app
  manifest. Search for distinctive function names and comments from shared crates.
- [ ] **No file-format logic.** The app crate does not parse or write measurement, CAD,
  program or report files itself; it calls the shared importers and exporters.
- [ ] **Exceptions are justified.** Any new `[[exception]]` in `apps-policy.toml` names the
  crate, the file, an approving maintainer and a reason, and the reason says when the file
  will move to a shared crate.
- [ ] **Same command, same meaning.** If the PR enables a command namespace in another app,
  the command keeps its ID, parameters, result and side-effect class (CONSTITUTION §3).
  New behaviour needs a new command or a new major version, not an app-specific variant.

When a check fails, ask for the logic to move to a shared crate (or to a new registry
command) rather than accepting a local fix.
