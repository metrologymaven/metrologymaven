#!/usr/bin/env python3
"""Generate the ECO.APPS.POLICY fixture workspaces (MIT OR Apache-2.0).

Each subfolder is a miniature Cargo workspace with path dependencies only (so it resolves
offline), plus layers.toml, apps.toml, apps-policy.toml, one registry manifest per app under
manifests/, apps-status.json, and expected.txt: the findings of `cargo xtask apps-policy`,
one per line, labelled by hand from the rules of spec/eco/apps/policy/NODE.md. They are not
produced by the Rust code. The only computation here is a sanity check that each hand
label's line number points at the text it claims to (see `assert_line`).

The crate sources are never compiled; only `cargo metadata` and the token scan read them.

Run:  python fixtures/policy/make_fixtures.py
"""

import json
import shutil
from pathlib import Path

HERE = Path(__file__).parent

DA = "sha256:" + "a" * 64
DB = "sha256:" + "b" * 64
DC = "sha256:" + "c" * 64
DD = "sha256:" + "d" * 64
DE = "sha256:" + "e" * 64

LAYERS = """\
# Fixture layer map (ECO.REPO.LAYERS shape).
order = ["model", "services", "app", "tool"]

[crates]
"fid-registry-macros" = "model"
"fid-x" = "services"
{extra}"""

APPS_TOML = """\
# Fixture app list (ECO.APPS.LIST shape).
[[app]]
id = "a"
job = "Fixture app a"
owner = "vacant"
phase = "P1a"
decision = "D-004"

[[app]]
id = "b"
job = "Fixture app b"
owner = "vacant"
phase = "P1a"
decision = "D-004"

[[app]]
id = "c"
job = "Fixture app c, no manifest yet"
owner = "vacant"
phase = "P2"
decision = "D-004"
"""

CONFIG = """\
# ECO.APPS.POLICY fixture config. The declaration-macro crate and markers are stand-ins
# until CORE.REG.DEF names the real ones.
macro_crates = ["fid-registry-macros"]
declaration_markers = ["#[fid_command", "fid_command!"]
identity_tokens = ["AppId::from", "AppId::new", "current_app", "FID_APP"]
"""

CONFIG_EXCEPTED = CONFIG + """
[[exception]]
crate = "fid-app-a"
file = "src/fit.rs"
approver = "example-maintainer"
reason = "Fixture: approved temporary extra file"
"""

MACROS_LIB = "//! Stand-in for the registry declaration-macro crate (CORE.REG.DEF, open).\n"

X_LIB = """\
//! Shared crate fid-x: declares a registry command; shared crates may.

use fid_registry_macros::fid_command;

#[fid_command(id = "x.do_thing@1")]
pub fn do_thing() -> u32 {
    1
}
"""

MAIN_20 = """\
//! fid-app-a: the entry point of app `a`. Policy lives in the manifest, not here.

use fid_x::do_thing;

/// Composition only: every behaviour comes from shared crates.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        println!("usage: fid-app-a");
        return;
    }
    let n = do_thing();
    println!("{n}");
}

// The window, panels and defaults come from the app manifest
// (ECO.APPS.MANIFEST), loaded by the shared shell.
//
// Nothing else belongs in this file: no measurement, fitting,
// evaluation or file-format logic.
"""


def main_lines(n, name):
    head = f"//! {name}: entry point.\n\nfn main() {{\n"
    tail = "}\n"
    body_lines = n - 4
    body = "".join(f"    println!(\"{i}\");\n" for i in range(body_lines))
    text = head + body + tail
    assert len(text.splitlines()) == n
    return text


def package(name, deps=(), dev_deps=()):
    lines = ["[package]", f'name = "{name}"', 'version = "0.0.0"', 'edition = "2021"', "publish = false", ""]
    lines.append("[dependencies]")
    for d, path in deps:
        lines.append(f'{d} = {{ path = "../{path}" }}')
    if dev_deps:
        lines += ["", "[dev-dependencies]"]
        for d, path in dev_deps:
            lines.append(f'{d} = {{ path = "../{path}" }}')
    return "\n".join(lines) + "\n"


def cmd(cid, digest, side="R0"):
    return {"id": cid, "definition_digest": digest, "side_effect": side}


def registry(app, commands):
    return {"app": app, "commands": sorted(commands, key=lambda c: c["id"])}


def status(apps):
    """apps: list of (id, commands or None for manifest: false)."""
    out = []
    for aid, cmds in sorted(apps):
        if cmds is None:
            out.append({"id": aid, "manifest": False})
        else:
            out.append({
                "id": aid,
                "manifest": True,
                "status": "alpha",
                "status_scope": "fixture",
                "known_gaps": [],
                "namespaces": sorted({c["id"].split(".")[0] for c in cmds}),
                "commands": sorted(cmds, key=lambda c: c["id"]),
                "branding_placeholder": True,
            })
    return {"header": {"generator": "make_fixtures.py", "version": "0"}, "apps": out}


def assert_line(text, line, needle):
    """Sanity check for a hand label: line `line` (1-based) contains `needle`."""
    got = text.splitlines()[line - 1]
    assert needle in got, (line, needle, got)


def write(root, rel, content):
    p = root / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    if isinstance(content, (dict, list)):
        content = json.dumps(content, indent=2, sort_keys=True) + "\n"
    p.write_text(content, encoding="utf-8", newline="\n")


def workspace(name, crates, layers_extra, expected, manifests=None, apps_status=None, extra_files=None):
    """crates: {dir: (Cargo.toml text, {rel path: source})}."""
    root = HERE / name
    if root.exists():
        shutil.rmtree(root)
    members = ", ".join(f'"{d}"' for d in sorted(crates))
    write(root, "Cargo.toml", f'[workspace]\nresolver = "2"\nmembers = [{members}]\n')
    write(root, "layers.toml", LAYERS.format(extra=layers_extra))
    write(root, "apps.toml", APPS_TOML)
    write(root, "apps-policy.toml", CONFIG)
    for d, (manifest, sources) in crates.items():
        write(root, f"{d}/Cargo.toml", manifest)
        for rel, src in sources.items():
            write(root, f"{d}/{rel}", src)
    if manifests is None:
        manifests = {
            "a": registry("a", [cmd("x.do_thing@1", DA)]),
            "b": registry("b", [cmd("x.do_thing@1", DA)]),
        }
    for app, m in manifests.items():
        write(root, f"manifests/{app}.json", m)
    if apps_status is None:
        apps_status = status([("a", [cmd("x.do_thing@1", DA)]), ("b", [cmd("x.do_thing@1", DA)]), ("c", None)])
    write(root, "apps-status.json", apps_status)
    for rel, content in (extra_files or {}).items():
        write(root, rel, content)
    write(root, "expected.txt", "".join(line + "\n" for line in expected))


MACROS = ("macros", (package("fid-registry-macros"), {"src/lib.rs": MACROS_LIB}))
X = ("x", (package("fid-x", [("fid-registry-macros", "macros")]), {"src/lib.rs": X_LIB}))
APP_A = ("app-a", (package("fid-app-a", [("fid-x", "x")]), {"src/main.rs": MAIN_20}))
APP_LAYER_A = '"fid-app-a" = "app"\n'

assert len(MAIN_20.splitlines()) == 20


def ok():
    workspace("ok", dict([MACROS, X, APP_A]), APP_LAYER_A, [])


DEFN_MAIN = """\
//! fid-app-a declaring its own command: not allowed (POLICY-DEFN).

use fid_registry_macros::fid_command;

// A comment that mentions #[fid_command] does not count.
const NOTE: &str = "#[fid_command] in a string does not count";

#[fid_command(id = "a.local@1")]
fn local() {}

fn main() {
    let _ = NOTE;
    local();
}

#[ fid_command(id = "a.spaced@1") ]
fn spaced() {}
"""


def defn():
    assert_line(DEFN_MAIN, 8, "#[fid_command(")
    assert_line(DEFN_MAIN, 16, "#[ fid_command(")
    app = ("app-a", (package("fid-app-a", [("fid-registry-macros", "macros"), ("fid-x", "x")]),
                     {"src/main.rs": DEFN_MAIN}))
    workspace("defn", dict([MACROS, X, app]), APP_LAYER_A, [
        "POLICY-DEFN fid-app-a Cargo.toml: depends on declaration-macro crate fid-registry-macros",
        "POLICY-DEFN fid-app-a src/main.rs:8: declaration marker #[fid_command",
        "POLICY-DEFN fid-app-a src/main.rs:16: declaration marker #[fid_command",
    ])


LEAK_LIB = """\
//! Shared crate fid-x. It must not know which app is running.

use fid_registry_macros::fid_command;

// current_app() in a comment does not count, nor does FID_APP.
/* Block comments neither: AppId::new("a") /* nested current_app() */ */
const HELP: &str = "set FID_APP to choose an app; \\"current_app()\\"";
const RAW: &str = r#"current_app() in a "raw" string"#;

#[fid_command(id = "x.do_thing@1")]
pub fn do_thing() -> bool {
    let app = crate::host::current_app();
    let recurrent_apples = HELP.len() + RAW.len() + current_app_name().len();
    let quote = '"';
    app == "a" && recurrent_apples > 0 && quote != 'x'
}

fn current_app_name() -> &'static str {
    "AppId::from"
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_only_code_may_name_the_app() {
        let _ = std::env::var(FID_APP);
        let _ = super::AppId::from("a");
    }
}

pub fn configured() -> bool {
    let from_env = std::env::var("FID_APP").is_ok();
    let at_build = option_env!( "FID_APP" ).is_some();
    from_env && at_build && HELP.is_empty()
}

#[path = "../../outside/extra.rs"]
mod extra;
"""

LEAK_TEST = """\
//! Integration tests of fid-x are test code: tokens here do not count.
#[test]
fn t() {
    let _ = fid_x::host::current_app();
}
"""

TOOL_MAIN = """\
//! A repository tool: tool crates are not shared product code, so neither rule applies.
fn main() {
    let _ = std::env::var(FID_APP);
}
"""


def leak():
    assert_line(LEAK_LIB, 12, "current_app()")
    assert_line(LEAK_LIB, 32, 'env::var("FID_APP")')
    assert_line(LEAK_LIB, 33, 'option_env!( "FID_APP" )')
    assert_line(LEAK_LIB, 37, '#[path = "../../outside/extra.rs"]')
    x = ("x", (package("fid-x", [("fid-registry-macros", "macros")]),
               {"src/lib.rs": LEAK_LIB, "tests/host.rs": LEAK_TEST}))
    y = ("y", (package("fid-y", [("fid-app-a", "app-a")]), {"src/lib.rs": "//! Depends on an app crate.\n"}))
    z = ("z", (package("fid-z", [], [("fid-app-a", "app-a")]),
               {"src/lib.rs": "//! Only a dev-dependency on an app crate: allowed.\n"}))
    tool = ("tool", (package("fid-tool", [("fid-app-a", "app-a")]), {"src/main.rs": TOOL_MAIN}))
    extra = APP_LAYER_A + '"fid-y" = "services"\n"fid-z" = "services"\n"fid-tool" = "tool"\n'
    workspace("leak", dict([MACROS, x, APP_A, y, z, tool]), extra, [
        "POLICY-LEAK fid-x src/lib.rs:12: uses app-identity token current_app",
        "POLICY-LEAK fid-x src/lib.rs:32: uses app-identity token FID_APP",
        "POLICY-LEAK fid-x src/lib.rs:33: uses app-identity token FID_APP",
        "POLICY-LEAK fid-x src/lib.rs:37: includes source outside src/, which the scan does not read: "
        "../../outside/extra.rs",
        "POLICY-LEAK fid-y Cargo.toml: depends on app crate fid-app-a",
    ])


def ident():
    manifests = {
        "a": registry("a", [cmd("x.do_thing@1", DA), cmd("y.only_a@1", DD), cmd("z.same@1", DC, "W2")]),
        # y.only_a@1 differs in b's registry but b does not enable it: no finding.
        "b": registry("b", [cmd("x.do_thing@1", DB), cmd("y.only_a@1", DE), cmd("z.same@1", DC, "W2")]),
    }
    st = status([
        ("a", [cmd("x.do_thing@1", DA), cmd("y.only_a@1", DD), cmd("z.same@1", DC, "W2")]),
        ("b", [cmd("x.do_thing@1", DA), cmd("z.same@1", DC, "W2")]),
        ("c", None),
    ])
    workspace("ident", dict([MACROS, X, APP_A]), APP_LAYER_A, [
        f"POLICY-IDENT x.do_thing@1: definition_digest differs between apps: a={DA} b={DB}",
    ], manifests=manifests, apps_status=st)


FIT_RS = """\
//! Fitting code in an app crate: belongs in a shared crate (POLICY-SIZE).
pub fn mean(xs: &[f64]) -> f64 {
    xs.iter().sum::<f64>() / xs.len() as f64
}
"""


def size():
    app = ("app-a", (package("fid-app-a", [("fid-x", "x")]), {"src/main.rs": MAIN_20, "src/fit.rs": FIT_RS}))
    workspace("size", dict([MACROS, X, app]), APP_LAYER_A, [
        "POLICY-SIZE fid-app-a src/fit.rs: Rust source file other than src/main.rs",
    ], extra_files={"apps-policy-excepted.toml": CONFIG_EXCEPTED})


APP_C_MAIN = """\
//! fid-app-c pulls code in from outside src/: hidden from the SIZE file listing.

include!("../../x/src/lib.rs");
// include!("../../x/src/lib.rs") in a comment does not count.

fn main() {}
"""


def size_long():
    assert_line(APP_C_MAIN, 3, "include!(")
    a = ("app-a", (package("fid-app-a", [("fid-x", "x")]), {"src/main.rs": main_lines(61, "fid-app-a")}))
    b = ("app-b", (package("fid-app-b", [("fid-x", "x")]), {"src/main.rs": main_lines(60, "fid-app-b")}))
    c = ("app-c", (package("fid-app-c", [("fid-x", "x")]), {"src/main.rs": APP_C_MAIN}))
    extra = APP_LAYER_A + '"fid-app-b" = "app"\n"fid-app-c" = "app"\n'
    workspace("size-long", dict([MACROS, X, a, b, c]), extra, [
        "POLICY-SIZE fid-app-a src/main.rs: 61 lines, limit 60",
        "POLICY-SIZE fid-app-c src/main.rs:3: includes source outside src/: ../../x/src/lib.rs",
    ])


def input_errors():
    manifests = {
        "a": registry("a", [cmd("x.do_thing@1", DA)]),
        "b": registry("b", [cmd("x.do_thing@1", DA)]),
    }
    st = status([
        ("a", [cmd("x.do_thing@1", DA)]),
        ("b", [cmd("w.missing@1", DC), cmd("x.do_thing@1", DA)]),
        ("c", None),
        ("d", [cmd("x.do_thing@1", DA)]),
    ])
    workspace("input", dict([MACROS, X, APP_A]), APP_LAYER_A, [
        "POLICY-INPUT b: effective command w.missing@1 is not in registry manifest b.json",
        "POLICY-INPUT d: app is not listed in apps.toml",
        "POLICY-INPUT d: no registry manifest d.json for an app with a manifest",
    ], manifests=manifests, apps_status=st)


if __name__ == "__main__":
    ok()
    defn()
    leak()
    ident()
    size()
    size_long()
    input_errors()
