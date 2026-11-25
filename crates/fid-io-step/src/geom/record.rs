//! Deterministic text records of a [`GeomMap`] (AC-5). One line per item, maps in ascending
//! entity number, every real written as the 16 lower-case hex digits of its bit pattern, so equal
//! results give equal bytes and any bit change shows.

use super::{Approx, Curve2, CurveRef, GeomMap, Pcurve2, SkipReason, SurfaceRef, Trim, TrimMaster};
use fid_geo_nurbs::{AnalyticSurface, NurbsCurve, NurbsSurface, Placement};
use std::fmt::Write;

struct Out(String);

impl Out {
    fn word(&mut self, w: &str) {
        self.0.push(' ');
        self.0.push_str(w);
    }

    fn int(&mut self, v: u64) {
        let _ = write!(self.0, " {v}");
    }

    fn real(&mut self, v: f64) {
        let _ = write!(self.0, " {:016x}", v.to_bits());
    }

    fn reals(&mut self, vs: &[f64]) {
        self.int(vs.len() as u64);
        for &v in vs {
            self.real(v);
        }
    }

    fn flag(&mut self, b: bool) {
        self.word(if b { "T" } else { "F" });
    }

    fn line(&mut self, tag: &str, n: u64) {
        if !self.0.is_empty() {
            self.0.push('\n');
        }
        self.0.push_str(tag);
        self.int(n);
    }

    fn placement(&mut self, p: &Placement) {
        for v in [p.origin(), p.axis(), p.ref_dir()] {
            for c in v {
                self.real(c);
            }
        }
    }

    fn nurbs_curve(&mut self, c: &NurbsCurve) {
        self.int(c.degree() as u64);
        self.reals(c.knots().as_slice());
        self.int(c.ctrl().len() as u64);
        for p in c.ctrl() {
            for &x in p {
                self.real(x);
            }
        }
        match c.weights() {
            Some(w) => {
                self.word("W");
                self.reals(w);
            }
            None => self.word("NW"),
        }
    }

    fn nurbs_surface(&mut self, s: &NurbsSurface) {
        self.int(s.degree_u() as u64);
        self.int(s.degree_v() as u64);
        self.reals(s.knots_u().as_slice());
        self.reals(s.knots_v().as_slice());
        let (nu, nv) = s.shape();
        self.int(nu as u64);
        self.int(nv as u64);
        for p in s.ctrl() {
            for &x in p {
                self.real(x);
            }
        }
        match s.weights() {
            Some(w) => {
                self.word("W");
                self.reals(w);
            }
            None => self.word("NW"),
        }
    }

    fn trim(&mut self, t: &Trim) {
        match t.param {
            Some(p) => {
                self.word("P");
                self.real(p);
            }
            None => self.word("NP"),
        }
        match t.point {
            Some(q) => {
                self.word("X");
                for c in q {
                    self.real(c);
                }
            }
            None => self.word("NX"),
        }
    }

    fn master(&mut self, m: TrimMaster) {
        self.word(match m {
            TrimMaster::Cartesian => "cartesian",
            TrimMaster::Parameter => "parameter",
            TrimMaster::Unspecified => "unspecified",
        });
    }
}

fn curve(o: &mut Out, n: u64, c: &CurveRef) {
    o.line("curve", n);
    match c {
        CurveRef::Line { line, magnitude_m } => {
            o.word("line");
            o.placement(line.placement());
            o.real(*magnitude_m);
        }
        CurveRef::Circle(c) => {
            o.word("circle");
            o.placement(c.placement());
            o.real(c.radius());
        }
        CurveRef::Ellipse(e) => {
            o.word("ellipse");
            o.placement(e.placement());
            let (a, b) = e.semi_axes();
            o.real(a);
            o.real(b);
        }
        CurveRef::Nurbs(c) => {
            o.word("nurbs");
            o.nurbs_curve(c);
        }
        CurveRef::Polyline(c) => {
            o.word("polyline");
            o.nurbs_curve(c);
        }
        CurveRef::Trimmed(t) => {
            o.word("trimmed");
            o.int(t.basis);
            o.trim(&t.trim_1);
            o.trim(&t.trim_2);
            o.flag(t.sense);
            o.master(t.master);
        }
        CurveRef::Surface { curve_3d, associated, master, seam } => {
            o.word(if *seam { "seam_curve" } else { "surface_curve" });
            o.int(*curve_3d);
            o.int(associated.len() as u64);
            for &a in associated {
                o.int(a);
            }
            o.master(*master);
        }
    }
}

fn surface(o: &mut Out, n: u64, s: &SurfaceRef) {
    o.line("surface", n);
    match s {
        SurfaceRef::Analytic(a) => {
            o.word(a.kind());
            match a {
                AnalyticSurface::Plane(p) => o.placement(p.placement()),
                AnalyticSurface::Cylinder(c) => {
                    o.placement(c.placement());
                    o.real(c.radius());
                }
                AnalyticSurface::Cone(c) => {
                    o.placement(c.placement());
                    o.real(c.radius());
                    o.real(c.half_angle());
                }
                AnalyticSurface::Sphere(s) => {
                    o.placement(s.placement());
                    o.real(s.radius());
                }
                AnalyticSurface::Torus(t) => {
                    o.placement(t.placement());
                    let (r1, r2) = t.radii();
                    o.real(r1);
                    o.real(r2);
                }
            }
        }
        SurfaceRef::Nurbs(s) => {
            o.word("nurbs");
            o.nurbs_surface(s);
        }
        SurfaceRef::Extrusion { nurbs, param_exact, domain_defaulted } => {
            o.word("extrusion");
            o.flag(*param_exact);
            o.flag(*domain_defaulted);
            o.nurbs_surface(nurbs);
        }
        SurfaceRef::Revolution { nurbs, param_exact, u_param_exact } => {
            o.word("revolution");
            o.flag(*param_exact);
            o.flag(*u_param_exact);
            o.nurbs_surface(nurbs);
        }
        SurfaceRef::Approximated(s) => {
            o.word("approximated");
            o.nurbs_surface(s);
        }
    }
}

fn pcurve(o: &mut Out, n: u64, p: &Pcurve2) {
    o.line("pcurve", n);
    o.int(p.surface);
    o.int(p.representation);
    o.int(p.curve_entity);
    o.real(p.scale[0]);
    o.real(p.scale[1]);
    o.flag(p.param_exact);
    match &p.curve {
        Curve2::Line { origin, vector } => {
            o.word("line");
            for v in origin.iter().chain(vector.iter()) {
                o.real(*v);
            }
        }
        Curve2::Conic { center, x_dir, semi_axes } => {
            o.word("conic");
            for v in center.iter().chain(x_dir.iter()).chain(semi_axes.iter()) {
                o.real(*v);
            }
        }
        Curve2::Nurbs(c) => {
            o.word("nurbs");
            o.nurbs_curve(c);
        }
    }
}

fn approx(o: &mut Out, n: u64, a: &Approx) {
    o.line("approx", n);
    o.word(a.status.code());
    o.real(a.max_dev_m);
    o.real(a.tolerance_m);
    o.int(a.samples);
    o.int(a.grid[0] as u64);
    o.int(a.grid[1] as u64);
    o.int(a.spans[0] as u64);
    o.int(a.spans[1] as u64);
    o.int(u64::from(a.refined));
    for v in [a.domain.u.0, a.domain.u.1, a.domain.v.0, a.domain.v.1] {
        o.real(v);
    }
    o.flag(a.domain_defaulted);
}

fn reason(o: &mut Out, r: &SkipReason) {
    o.word(r.code());
    match r {
        SkipReason::Unsupported { at } => o.int(*at),
        SkipReason::Malformed { at, what } => {
            o.int(*at);
            o.word(what);
        }
        SkipReason::NurbsInvalid { at, err } => {
            o.int(*at);
            o.word(err.code());
        }
        SkipReason::GeoInvalid { at, err } => {
            o.int(*at);
            o.word(err.code());
        }
        SkipReason::Dangling { to } => o.int(*to),
        _ => {}
    }
}

pub(super) fn write(m: &GeomMap) -> String {
    let mut o = Out(String::new());
    for (&n, p) in &m.points {
        o.line("point", n);
        for &c in p {
            o.real(c);
        }
    }
    for (&n, c) in &m.curves {
        curve(&mut o, n, c);
    }
    for (&n, s) in &m.surfaces {
        surface(&mut o, n, s);
    }
    for (&n, p) in &m.pcurves {
        pcurve(&mut o, n, p);
    }
    for (&n, a) in &m.approximations {
        approx(&mut o, n, a);
    }
    for s in &m.skipped {
        o.line("skipped", s.entity);
        o.word(&s.type_name);
        reason(&mut o, &s.reason);
    }
    for w in &m.warnings {
        o.line("warning", w.entity);
        o.word(w.code);
        o.real(w.value);
    }
    o.0.push('\n');
    o.0
}
