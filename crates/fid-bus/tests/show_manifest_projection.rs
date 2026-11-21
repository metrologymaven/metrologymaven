//! Regression test: registry visibility of `registry.show_manifest@1` (CORE.REG.MAN, D-035)
//! agrees with the bus. `show_manifest` uses the shared `fid_registry::validate::mcp_projection`
//! (#394), which the bus re-exports; over every combination of side-effect class, declared
//! projection, `human_only` and exposure set, "an agent sees it" (`manifest::sees`) must equal
//! "the bus's MCP projection check does not refuse it" (`authorise_projection`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::policy::projection::{authorise_projection, mcp_projection as bus_projection};
use fid_bus::principal::ChannelKind;
use fid_registry::manifest::{command_def, mcp_projection, sees, Audience};
use fid_registry::{Projection, SideEffect, Surface};
use std::collections::BTreeSet;

#[test]
fn show_manifest_projection_equals_the_bus_rule_on_every_combination() {
    let base = command_def().unwrap();
    let surfaces = Surface::ALL;
    assert_eq!(surfaces.len(), 5);
    let mut n = 0;
    for side_effect in SideEffect::ALL {
        for projection in Projection::ALL {
            for human_only in [false, true] {
                for mask in 0u32..(1 << surfaces.len()) {
                    let mut def = base.clone();
                    def.side_effect = *side_effect;
                    def.mcp_projection = *projection;
                    def.human_only = human_only;
                    def.exposure = surfaces
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| mask & (1 << i) != 0)
                        .map(|(_, s)| *s)
                        .collect::<BTreeSet<_>>();
                    let what = format!("{side_effect:?} {projection:?} human_only={human_only} mask={mask}");
                    assert_eq!(mcp_projection(&def), bus_projection(&def), "{what}");
                    let refused = authorise_projection(ChannelKind::Mcp, &def).is_err();
                    assert_eq!(sees(Audience::Agent, &def), !refused, "{what}");
                    assert!(sees(Audience::Person, &def), "{what}");
                    n += 1;
                }
            }
        }
    }
    assert_eq!(n, SideEffect::ALL.len() * Projection::ALL.len() * 2 * 32);
}
