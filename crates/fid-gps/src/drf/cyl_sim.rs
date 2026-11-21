//! The measured path of a DRF with planar **and cylindrical** datum features (spec node
//! `GDT.DRF.BUILD`, P1a at RMB): from the measured points of one to three datum features, in
//! precedence order, to certified simulated datums (planes and axes) and the DRF
//! [`build_drf`] assembles from them.
//!
//! Planes are simulated exactly as in [`super::plane_sim`] (its tangential planes and its
//! orientation table). An **external** cylinder (a shaft, a boss, a pin) is simulated by the
//! minimum circumscribed cylinder of `fid-fit` (`KRN.ENV.MCC.CYL`, `fit.cylinder.mc@1.0.0`):
//! [`fit_mcc_cylinder`] followed by [`to_fit_result`], which is exactly what the `Fit` trait's
//! `MccCylinderFit::fit` does, so the datum geometry is the trait's output and the trait's
//! fail-closed gate applies (a fit with `local_optimum_only`, `suspect_axis`, or a strict local
//! minimum whose global cross-check did not pass is `flagged_fit{label}`). The full record is
//! kept only as evidence, and it is re-checked here too (defence in depth): a free direction
//! must be a certified strict local minimum with a `passed` cross-check, a fixed direction the
//! certified global minimum of the convex problem, and no certificate may carry
//! `local_optimum_only`. Every other `fid-fit` refusal passes through typed as
//! `simulation_failed{label:code}`; an oval cross-section, whose envelope has a degenerate
//! active set that the certifier does not support, is `simulation_failed{label:solver}`.
//!
//! An **internal** cylinder (a bore) needs the maximum inscribed cylinder (`KRN.ENV.MIC.CYL`),
//! which is not built: it is refused before anything is fitted,
//! `unsupported_datum_type_p1a{label}`; no axis is ever made up for it.
//!
//! # Orientation constraint per datum (pinned)
//!
//! As for planes, the constraint follows from the DoFs the predecessors left free and from the
//! datum's **nominal** direction (a plane's outward normal, a cylinder's axis direction)
//! compared with the predecessors' nominals within [`ANGLE_TOL`](super::build::ANGLE_TOL);
//! `nz` is the primary's nominal:
//!
//! | free rotations before it | nominal relationship | plane | cylinder |
//! |---|---|---|---|
//! | all (primary) | any | free normal, certified | free direction, certified, cross-checked |
//! | `rz` only | parallel to `nz` | normal fixed at `+-z` | direction fixed at `+-z` (global) |
//! | `rz` only | perpendicular to `nz` | normal perpendicular to `z`, free about it | refused, `orientation_constraint_unsupported{label}` |
//! | none | parallel to `nz`, or (secondary a plane) to its `y` or `x` | normal fixed at that measured axis | direction fixed at that measured axis (global) |
//! | none, after two parallel axes | not parallel to `nz` | refused, `orientation_constraint_unsupported{label}` | same |
//! | any | any other angle | refused, `oblique_datum_unsupported{label}` | same |
//!
//! "Fixed" directions are signed by the nominal. A cylinder whose axis would have to stay
//! perpendicular to `z` but rotate about it (a cross hole or a horizontal shaft after a primary
//! plane) is refused because `KRN.ENV.MCC.CYL` takes a free or a fully fixed direction only.
//! The simulated axis is the fit's axis through the foot of the points' centroid, its direction
//! signed by the nominal.
//!
//! # Determinism
//!
//! D0: `fid-fit`'s envelope with the pinned seed [`MCC_SEED`] (eight seeded starts for a free
//! direction, one for a fixed one), and the planar path's own D0 rules.

use super::build::{build_drf, DatumKind, Dof, DrfError, SimGeometry, SimulatedDatum, MAX_DATUMS, UNIT_TOL};
use super::plane_sim::{
    about_z, cross, norm, parallel, perpendicular, plane_of, reject, sign_toward, unit, variant_of, Ctx, DatumEvidence,
    MeasuredDrf, Orientation, SimError, V3,
};
use crate::dmt::{AlgId, DmtKey};
use fid_fit::contract::{Centred, Constraint, FitError, FitParams, FitResult, FittedFeature, Method, PointsView};
use fid_fit::mcc_cyl::{fit_mcc_cylinder, to_fit_result, CrossOutcome, MccCylinder, Optimality as MccOptimality};
use fid_fit::tan_rock::TanVariant;
use fid_types::Digest;

/// The seed of every cylinder fit (recorded in the evidence).
pub const MCC_SEED: u64 = 0;

/// The algorithm ID of the cylinder simulator, `fit.cylinder.mc@1.0.0` (`fid-fit`'s ID).
pub fn mcc_algorithm_id() -> String {
    fid_fit::mcc_cyl::algorithm().to_string()
}

/// The material side of a cylindrical datum feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CylinderSide {
    /// A shaft, boss or pin: material inside (simulated by the minimum circumscribed cylinder).
    External,
    /// A bore or hole: material outside (needs the maximum inscribed cylinder, not built).
    Internal,
}

impl CylinderSide {
    /// `external` or `internal`.
    pub fn as_str(self) -> &'static str {
        match self {
            CylinderSide::External => "external",
            CylinderSide::Internal => "internal",
        }
    }

    /// Parses `external` or `internal`.
    pub fn parse(s: &str) -> Option<CylinderSide> {
        match s {
            "external" => Some(CylinderSide::External),
            "internal" => Some(CylinderSide::Internal),
            _ => None,
        }
    }
}

/// What a measured datum feature is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeasuredKind {
    /// A planar face; its nominal is the outward unit normal.
    Plane,
    /// A cylindrical feature; its nominal is the unit axis direction (its sign is the datum
    /// axis's sense).
    Cylinder(CylinderSide),
}

/// One measured datum feature.
#[derive(Debug, Clone, Copy)]
pub struct MeasuredFeature<'a> {
    /// The datum label (`A`, `B`, ...).
    pub label: &'a str,
    /// The measured points as contiguous `x, y, z` triples (m, part frame).
    pub xyz: &'a [f64],
    /// The nominal direction from the CAD model (module documentation): it gives the basic
    /// relationship to the other datums (and, for a plane, the material-free side).
    pub nominal: [f64; 3],
    /// Plane or cylinder.
    pub kind: MeasuredKind,
}

/// The simulators the DMT row's chain names: at most one tangential plane and at most one
/// cylinder envelope.
#[derive(Debug, Clone)]
pub struct Simulators {
    plane: Option<(AlgId, TanVariant)>,
    cylinder: Option<AlgId>,
}

impl Simulators {
    /// From the algorithm IDs of the chain's association steps: each must be one of `fid-fit`'s
    /// tangential planes (`fit.plane.tan_*@1.0.0`) or its minimum circumscribed cylinder
    /// (`fit.cylinder.mc@1.0.0`), at most one of each (`unsupported_simulator{alg}` otherwise,
    /// naming the offending or repeated step; `unsupported_simulator{none}` for no step).
    pub fn from_association(algs: &[&AlgId]) -> Result<Simulators, SimError> {
        if algs.is_empty() {
            return Err(SimError::UnsupportedSimulator { alg: "none".to_owned() });
        }
        let mcc = mcc_algorithm_id();
        let mut plane: Option<(AlgId, TanVariant)> = None;
        let mut cylinder: Option<AlgId> = None;
        for alg in algs {
            let bad = || SimError::UnsupportedSimulator { alg: alg.as_str().to_owned() };
            if let Some(v) = variant_of(alg) {
                if plane.is_some() {
                    return Err(bad());
                }
                plane = Some(((*alg).clone(), v));
            } else if alg.as_str() == mcc {
                if cylinder.is_some() {
                    return Err(bad());
                }
                cylinder = Some((*alg).clone());
            } else {
                return Err(bad());
            }
        }
        Ok(Simulators { plane, cylinder })
    }

    /// The plane simulator, if the chain names one.
    pub fn plane(&self) -> Option<&AlgId> {
        self.plane.as_ref().map(|(a, _)| a)
    }

    /// The cylinder simulator, if the chain names one.
    pub fn cylinder(&self) -> Option<&AlgId> {
        self.cylinder.as_ref()
    }
}

/// The orientation the predecessors impose on a datum.
#[derive(Debug, Clone, Copy)]
enum Hold {
    Free,
    Fixed(V3),
    AboutZ(V3),
}

fn fit_error(label: &str, e: FitError) -> SimError {
    match e {
        FitError::FlaggedFit { .. } => SimError::FlaggedFit { label: label.to_owned() },
        other => SimError::Fit { label: label.to_owned(), code: other.code() },
    }
}

/// The `Fit` trait's gate on an envelope record ([`to_fit_result`], with `scale` the points'
/// centring scale), then the re-check of the module documentation for a free (`fixed` false)
/// or fixed direction. Returns the accepted contract result and the evidence; any flag, an
/// unchecked or failed cross-check, or an optimality that does not match the orientation is
/// `flagged_fit{label}`.
pub fn accept_cylinder(
    label: &str,
    m: &MccCylinder,
    scale: f64,
    fixed: bool,
) -> Result<(FitResult, DatumEvidence), SimError> {
    let accepted = to_fit_result(m.clone(), scale).map_err(|e| fit_error(label, e))?;
    let flagged = || SimError::FlaggedFit { label: label.to_owned() };
    if m.local_optimum_only || m.suspect_axis || m.certificate.flags().local_optimum_only {
        return Err(flagged());
    }
    let (orientation, optimality, check) = match (fixed, m.optimality) {
        (false, MccOptimality::StrictLocal) => match m.cross_check {
            Some(c) if m.cross_check_passed && c.outcome == CrossOutcome::Passed => {
                (Orientation::Free, "strict_local_cross_checked", Some(c))
            }
            _ => return Err(flagged()),
        },
        (true, MccOptimality::Global) => (Orientation::Fixed, "global", None),
        _ => return Err(flagged()),
    };
    let (p, d, r) = match &accepted.feature {
        FittedFeature::Cylinder { p, d, r } => (*p, *d, *r),
        _ => return Err(SimError::Fit { label: label.to_owned(), code: "not_a_cylinder" }),
    };
    let cert = *m.certificate.digest();
    let cross_check_work = check.map(|c| [c.caps, c.full_tests, c.depth]);
    let alg = m.algorithm.to_string();
    let identity = Identity {
        alg: &alg,
        seed: MCC_SEED,
        optimality,
        orientation: orientation.as_str(),
        max_violation: m.max_violation,
        cross_check: check.map(|c| (c.outcome.as_str(), [c.caps, c.full_tests, c.depth])),
    };
    let ev = DatumEvidence {
        orientation,
        optimality,
        certificate: Some(cert),
        local_optimum_only: false,
        fit_digest: cylinder_digest(p, d, r, &m.contacts, &cert, &identity),
        points: m.residuals.len() as u32,
        candidates: 1,
        objective: r,
        max_violation: m.max_violation,
        contacts: m.contacts.clone(),
        cross_check: check.map(|c| c.outcome.as_str()),
        cross_check_work,
        seed: Some(MCC_SEED),
    };
    Ok((accepted, ev))
}

/// How an envelope was obtained and accepted, beyond its geometry (hashed by
/// [`cylinder_digest`]).
struct Identity<'a> {
    alg: &'a str,
    seed: u64,
    optimality: &'a str,
    orientation: &'a str,
    max_violation: f64,
    /// The cross-check outcome and its work: cap tests, full tests, deepest level.
    cross_check: Option<(&'a str, [u32; 3])>,
}

/// SHA-256 of `fid.gdt-drf-cylinder-fit/2\n`, then, each text followed by `\n`: the algorithm
/// ID, the seed (decimal), the optimality, the orientation, the cross-check outcome (`none` for
/// a fixed direction); then as little-endian bits: the axis point, direction and radius and the
/// largest violation (`f64`), the cross-check's cap tests, full tests and depth (`u32`, zeros
/// for none), the contact count and contacts (`u32`); then the certificate digest text. The
/// selected envelope's identity (`fid-fit` has no output digest for it yet): a change in the
/// algorithm, seed, acceptance or the way the cross-check passed changes it, so replay sees it.
fn cylinder_digest(p: V3, d: V3, r: f64, contacts: &[u32], cert: &Digest, id: &Identity<'_>) -> Digest {
    let mut b = Vec::with_capacity(160 + 4 * contacts.len() + 80);
    b.extend_from_slice(b"fid.gdt-drf-cylinder-fit/2\n");
    let seed = id.seed.to_string();
    let outcome = id.cross_check.map_or("none", |(o, _)| o);
    for t in [id.alg, seed.as_str(), id.optimality, id.orientation, outcome] {
        b.extend_from_slice(t.as_bytes());
        b.push(b'\n');
    }
    for x in p.iter().chain(d.iter()).chain([r, id.max_violation].iter()) {
        b.extend_from_slice(&x.to_bits().to_le_bytes());
    }
    let work = id.cross_check.map_or([0; 3], |(_, w)| w);
    for w in work {
        b.extend_from_slice(&w.to_le_bytes());
    }
    b.extend_from_slice(&(contacts.len() as u32).to_le_bytes());
    for c in contacts {
        b.extend_from_slice(&c.to_le_bytes());
    }
    b.extend_from_slice(cert.as_str().as_bytes());
    Digest::from_bytes(fid_canon::digest_bytes(&b).0)
}

/// The fit parameters of an envelope: free, or along the fixed direction `fixed`.
pub fn cylinder_params(fixed: Option<V3>) -> FitParams {
    let mut fp = FitParams::new(Method::MinimumCircumscribed);
    if let Some(d) = fixed {
        fp.constraints.push(Constraint::FixedAxisDirection(d));
    }
    fp
}

/// The envelope of one external cylinder, free (`fixed` `None`) or along a fixed direction:
/// [`fit_mcc_cylinder`] with [`cylinder_params`] and [`MCC_SEED`], then [`accept_cylinder`].
/// Its contract result is the `Fit` trait's (`MccCylinderFit::fit` on the same points,
/// parameters and seed; pinned by a test).
pub fn simulate_cylinder(label: &str, xyz: &[f64], fixed: Option<V3>) -> Result<(FitResult, DatumEvidence), SimError> {
    let pts = PointsView::new(xyz, None).map_err(|e| fit_error(label, e))?;
    let m = fit_mcc_cylinder(&pts, &cylinder_params(fixed), Some(MCC_SEED)).map_err(|e| fit_error(label, e))?;
    let scale = Centred::new(&pts).map_err(|e| fit_error(label, e))?.map().scale();
    accept_cylinder(label, &m, scale, fixed.is_some())
}

fn fit_cylinder(label: &str, xyz: &[f64], fixed: Option<V3>) -> Result<(V3, V3, DatumEvidence), SimError> {
    let (accepted, ev) = simulate_cylinder(label, xyz, fixed)?;
    match accepted.feature {
        FittedFeature::Cylinder { p, d, .. } => Ok((p, d, ev)),
        _ => Err(SimError::Fit { label: label.to_owned(), code: "not_a_cylinder" }),
    }
}

/// Simulates every datum in precedence order and builds the DRF (module documentation). `sims`
/// are the simulators of the resolved DMT row's chain and `row` its key; each datum records the
/// simulator of its kind and the row. Pure and deterministic; every outcome is a returned
/// `Result`.
pub fn build_measured_drf(
    features: &[MeasuredFeature<'_>],
    sims: &Simulators,
    row: &DmtKey,
) -> Result<MeasuredDrf, SimError> {
    if features.is_empty() {
        return Err(DrfError::NoDatums.into());
    }
    if features.len() > MAX_DATUMS {
        return Err(DrfError::TooManyDatums { count: features.len() }.into());
    }
    // Before anything is fitted: every nominal unit, every datum type supported, and its
    // simulator named by the chain.
    let none = || SimError::UnsupportedSimulator { alg: "none".to_owned() };
    let mut nominals: Vec<V3> = Vec::with_capacity(features.len());
    for f in features {
        let invalid = || SimError::NominalInvalid { label: f.label.to_owned() };
        let nn = norm(f.nominal).ok_or_else(invalid)?;
        if (nn - 1.0).abs() > UNIT_TOL {
            return Err(invalid());
        }
        nominals.push(unit(f.nominal).ok_or_else(invalid)?);
        match f.kind {
            MeasuredKind::Plane if sims.plane.is_none() => return Err(none()),
            MeasuredKind::Cylinder(CylinderSide::Internal) => {
                return Err(SimError::UnsupportedDatumType { label: f.label.to_owned() })
            }
            MeasuredKind::Cylinder(CylinderSide::External) if sims.cylinder.is_none() => return Err(none()),
            _ => {}
        }
    }
    let mut datums: Vec<SimulatedDatum> = Vec::new();
    let mut evidence = Vec::new();
    for (i, f) in features.iter().enumerate() {
        let nominal = nominals[i];
        let oblique = || SimError::ObliqueDatum { label: f.label.to_owned() };
        let hold = if datums.is_empty() {
            Hold::Free
        } else {
            let prefix = build_drf(&datums)?;
            let [x, y, z] = prefix.axes();
            let nz = nominals[0];
            if prefix.free().contains(Dof::Rz) {
                if parallel(nominal, nz) {
                    Hold::Fixed(z)
                } else if perpendicular(nominal, nz) {
                    Hold::AboutZ(z)
                } else {
                    return Err(oblique());
                }
            } else if parallel(nominal, nz) {
                Hold::Fixed(z)
            } else {
                // Only a secondary plane gives the nominal y (its normal made perpendicular to
                // the primary's nominal); after two parallel axes nothing else is nominal, so no
                // orientation can be held for a datum not parallel to them.
                if datums.get(1).map(|d| d.geometry.kind()) != Some(DatumKind::Plane) {
                    return Err(SimError::OrientationUnsupported { label: f.label.to_owned() });
                }
                let ny = unit(reject(nominals[1], nz)).ok_or_else(oblique)?;
                let nx = cross(ny, nz);
                if parallel(nominal, ny) {
                    Hold::Fixed(y)
                } else if parallel(nominal, nx) {
                    Hold::Fixed(x)
                } else {
                    return Err(oblique());
                }
            }
        };
        let (geometry, ev, alg) = match f.kind {
            MeasuredKind::Plane => {
                let (alg, variant) = sims.plane.as_ref().ok_or_else(none)?;
                let ctx = Ctx { label: f.label, variant: *variant };
                let pts = PointsView::new(f.xyz, None).map_err(|e| ctx.fit_err(e.code()))?;
                let (r, ev) = match hold {
                    Hold::Free => {
                        let r = ctx.fit(&pts, None, nominal)?;
                        let ev = ctx.evidence(&r, Orientation::Free, "strict_local", 1)?;
                        (r, ev)
                    }
                    Hold::Fixed(axis) => {
                        let r = ctx.fit(&pts, Some(sign_toward(axis, nominal)), nominal)?;
                        let ev = ctx.evidence(&r, Orientation::Fixed, "closed_form", 1)?;
                        (r, ev)
                    }
                    Hold::AboutZ(z) => {
                        let (r, tried) = about_z(&ctx, &pts, f.xyz, z, nominal)?;
                        let ev = ctx.evidence(&r, Orientation::AboutZ, "closed_form_enumerated", tried)?;
                        (r, ev)
                    }
                };
                (plane_of(f.label, &r)?, ev, alg.clone())
            }
            MeasuredKind::Cylinder(_) => {
                let alg = sims.cylinder.as_ref().ok_or_else(none)?;
                let fixed = match hold {
                    Hold::Free => None,
                    Hold::Fixed(axis) => Some(sign_toward(axis, nominal)),
                    Hold::AboutZ(_) => return Err(SimError::OrientationUnsupported { label: f.label.to_owned() }),
                };
                let (point, d, ev) = fit_cylinder(f.label, f.xyz, fixed)?;
                (SimGeometry::Axis { point, direction: sign_toward(d, nominal) }, ev, alg.clone())
            }
        };
        datums.push(SimulatedDatum { label: f.label.to_owned(), geometry, alg, dmt_row: row.clone() });
        evidence.push(ev);
    }
    let drf = build_drf(&datums)?;
    Ok(MeasuredDrf { drf, datums, evidence })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn simulators_from_the_chain() {
        let p = AlgId::parse("fit.plane.tan_minimax@1.0.0").unwrap();
        let c = AlgId::parse("fit.cylinder.mc@1.0.0").unwrap();
        let s = Simulators::from_association(&[&p, &c]).unwrap();
        assert_eq!(s.plane(), Some(&p));
        assert_eq!(s.cylinder(), Some(&c));
        let s = Simulators::from_association(&[&c]).unwrap();
        assert!(s.plane().is_none());
        let e = |algs: &[&AlgId]| Simulators::from_association(algs).unwrap_err().to_string();
        assert_eq!(e(&[]), "unsupported_simulator{none}");
        assert_eq!(e(&[&c, &c]), "unsupported_simulator{fit.cylinder.mc@1.0.0}");
        let l1 = AlgId::parse("fit.plane.tan_l1@1.0.0").unwrap();
        assert_eq!(e(&[&p, &l1]), "unsupported_simulator{fit.plane.tan_l1@1.0.0}");
        let ls = AlgId::parse("fit.cylinder.ls@1.0.0").unwrap();
        assert_eq!(e(&[&p, &ls]), "unsupported_simulator{fit.cylinder.ls@1.0.0}");
        assert_eq!(CylinderSide::parse("internal"), Some(CylinderSide::Internal));
        assert_eq!(CylinderSide::parse("bore"), None);
    }
}
