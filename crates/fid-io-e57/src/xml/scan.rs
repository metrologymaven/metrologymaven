//! Scan descriptions (`data3D` children) taken from the tree, and their tree form for the
//! writer.

use fid_xml::XmlText;

use super::tree::{find, E57Node, E57Tree, Key, Precision, XmlSectionError, XmlWarning};
use super::write::FORMAT_NAME;
use super::QUATERNION_TOL;

/// A scan's pose: rotation quaternion `q = [w, x, y, z]` and translation `t = [x, y, z]`
/// in metres, as stored (the quaternion is not normalised).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    /// Rotation quaternion `[w, x, y, z]`.
    pub q: [f64; 4],
    /// Translation `[x, y, z]`.
    pub t: [f64; 3],
}

impl Pose {
    /// The quaternion norm `sqrt(w*w + x*x + y*y + z*z)`: binary64, every operation rounded
    /// once, summed left to right, no FMA.
    pub fn quaternion_norm(&self) -> f64 {
        let [w, x, y, z] = self.q;
        (w * w + x * x + y * y + z * z).sqrt()
    }
}

/// Where a scan's points are: the `points` compressed vector's `fileOffset` (a physical
/// offset) and `recordCount`. Checked against the file by the binary layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectionRef {
    /// Physical offset of the compressed-vector section.
    pub offset: u64,
    /// Number of records.
    pub records: u64,
}

/// The type and attributes of one prototype field, exactly as present (`None`: absent).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FieldKind {
    /// An integer field.
    Integer {
        /// `minimum`.
        minimum: Option<i64>,
        /// `maximum`.
        maximum: Option<i64>,
    },
    /// A scaled-integer field.
    ScaledInteger {
        /// `minimum` of the raw value.
        minimum: Option<i64>,
        /// `maximum` of the raw value.
        maximum: Option<i64>,
        /// `scale`.
        scale: Option<f64>,
        /// `offset`.
        offset: Option<f64>,
    },
    /// A real field.
    Float {
        /// `precision`.
        precision: Option<Precision>,
        /// `minimum`.
        minimum: Option<f64>,
        /// `maximum`.
        maximum: Option<f64>,
    },
    /// A string field.
    String,
}

/// One field of a scan's point prototype.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldDesc {
    /// The field name (for example `cartesianX`, or a prefixed extension field).
    pub name: Key,
    /// Its type and attributes.
    pub kind: FieldKind,
}

/// `cartesianBounds`, each `[minimum, maximum]` with `minimum <= maximum`, in metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CartesianBounds {
    /// `xMinimum`, `xMaximum`.
    pub x: [f64; 2],
    /// `yMinimum`, `yMaximum`.
    pub y: [f64; 2],
    /// `zMinimum`, `zMaximum`.
    pub z: [f64; 2],
}

/// `sphericalBounds`: range and elevation as `[minimum, maximum]` (ordered), azimuth as
/// `[start, end]` (not ordered: the interval may wrap).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SphericalBounds {
    /// `rangeMinimum`, `rangeMaximum` in metres.
    pub range: [f64; 2],
    /// `elevationMinimum`, `elevationMaximum` in radians.
    pub elevation: [f64; 2],
    /// `azimuthStart`, `azimuthEnd` in radians.
    pub azimuth: [f64; 2],
}

/// A numeric limit value as stored, with its element type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Scalar {
    /// An `Integer` value.
    Integer(i64),
    /// A `ScaledInteger`: raw value, `scale` and `offset` as present.
    ScaledInteger {
        /// The raw value.
        raw: i64,
        /// `scale`.
        scale: Option<f64>,
        /// `offset`.
        offset: Option<f64>,
    },
    /// A `Float` value.
    Float(f64),
}

/// A `[minimum, maximum]` limit pair of one kind with `minimum <= maximum` (scaled integers:
/// equal scale and offset, raw values ordered).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScalarRange {
    /// The minimum.
    pub minimum: Scalar,
    /// The maximum.
    pub maximum: Scalar,
}

/// `colorLimits`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorLimits {
    /// `colorRedMinimum`, `colorRedMaximum`.
    pub red: ScalarRange,
    /// `colorGreenMinimum`, `colorGreenMaximum`.
    pub green: ScalarRange,
    /// `colorBlueMinimum`, `colorBlueMaximum`.
    pub blue: ScalarRange,
}

/// One `data3D` scan as the binary and import layers need it (REQ-001). Strings are
/// untrusted.
#[derive(Debug, Clone, PartialEq)]
pub struct ScanDesc {
    /// `guid` (required).
    pub guid: XmlText,
    /// `name`.
    pub name: Option<XmlText>,
    /// `description`.
    pub description: Option<XmlText>,
    /// `pose` (both `rotation` and `translation` are required when `pose` is present).
    pub pose: Option<Pose>,
    /// The `points` section.
    pub points: SectionRef,
    /// The `points` prototype's fields, in document order (at least one).
    pub prototype: Vec<FieldDesc>,
    /// Number of children of the `points` `codecs` vector (0 when absent).
    pub codecs: u64,
    /// `cartesianBounds`.
    pub cartesian_bounds: Option<CartesianBounds>,
    /// `sphericalBounds`.
    pub spherical_bounds: Option<SphericalBounds>,
    /// `intensityLimits` (`intensityMinimum`, `intensityMaximum`).
    pub intensity_limits: Option<ScalarRange>,
    /// `colorLimits`.
    pub color_limits: Option<ColorLimits>,
}

type Described = (Vec<ScanDesc>, Option<u64>, Vec<XmlWarning>);

/// The scans, the `images2D` count and the warnings of a tree.
pub(crate) fn describe(tree: &E57Tree) -> Result<Described, XmlSectionError> {
    let content = |reason, scan| XmlSectionError::Content { reason, scan };
    match find(&tree.root, "formatName") {
        Some(E57Node::String(t)) if t.raw() == FORMAT_NAME => {}
        Some(_) => return Err(content("format_name_wrong", None)),
        None => return Err(content("format_name_missing", None)),
    }
    match find(&tree.root, "versionMajor") {
        Some(E57Node::Integer { value: Some(1), .. }) => {}
        Some(_) => return Err(content("version_major_unsupported", None)),
        None => return Err(content("version_major_missing", None)),
    }
    let images2d = match find(&tree.root, "images2D") {
        None => None,
        Some(E57Node::Vector { children, .. }) => Some(children.len() as u64),
        Some(_) => return Err(content("images2d_not_vector", None)),
    };
    let children = match find(&tree.root, "data3D") {
        Some(E57Node::Vector { children, .. }) => children,
        Some(_) => return Err(content("data3d_not_vector", None)),
        None => return Err(content("data3d_missing", None)),
    };
    let mut scans = Vec::with_capacity(children.len());
    let mut warnings = Vec::new();
    for (i, c) in children.iter().enumerate() {
        let s = scan_desc(c).map_err(|r| content(r, Some(i)))?;
        if let Some(p) = &s.pose {
            let norm = p.quaternion_norm();
            let off = (norm - 1.0).abs();
            if off.is_nan() || off > QUATERNION_TOL {
                warnings.push(XmlWarning::QuaternionNotUnit { scan: i, norm });
            }
        }
        scans.push(s);
    }
    if scans.is_empty() {
        warnings.push(XmlWarning::NoScans);
    }
    Ok((scans, images2d, warnings))
}

fn opt_string(ch: &[(Key, E57Node)], name: &str) -> Result<Option<XmlText>, &'static str> {
    match find(ch, name) {
        None => Ok(None),
        Some(E57Node::String(t)) => Ok(Some(t.clone())),
        Some(_) => Err("scan_string_type"),
    }
}

/// The value of `Float` child `name`; `missing` when the child is absent, `wrong` when it is
/// not a `Float`, `value_missing` when it has no value.
fn float_child(
    ch: &[(Key, E57Node)],
    name: &str,
    missing: &'static str,
    wrong: &'static str,
) -> Result<f64, &'static str> {
    match find(ch, name) {
        Some(E57Node::Float { value: Some(v), .. }) => Ok(*v),
        Some(E57Node::Float { value: None, .. }) => Err("value_missing"),
        Some(_) => Err(wrong),
        None => Err(missing),
    }
}

fn structure<'a>(node: &'a E57Node, wrong: &'static str) -> Result<&'a [(Key, E57Node)], &'static str> {
    match node {
        E57Node::Structure(ch) => Ok(ch),
        _ => Err(wrong),
    }
}

fn pose(node: &E57Node) -> Result<Pose, &'static str> {
    let ch = structure(node, "pose_type")?;
    let rot = structure(find(ch, "rotation").ok_or("pose_incomplete")?, "pose_type")?;
    let tr = structure(find(ch, "translation").ok_or("pose_incomplete")?, "pose_type")?;
    let f = |c, n| float_child(c, n, "pose_incomplete", "pose_type");
    Ok(Pose { q: [f(rot, "w")?, f(rot, "x")?, f(rot, "y")?, f(rot, "z")?], t: [f(tr, "x")?, f(tr, "y")?, f(tr, "z")?] })
}

fn ordered(lo: f64, hi: f64) -> Result<[f64; 2], &'static str> {
    if lo <= hi {
        Ok([lo, hi])
    } else {
        Err("bounds_inverted")
    }
}

fn cartesian(node: &E57Node) -> Result<CartesianBounds, &'static str> {
    let ch = structure(node, "bounds_type")?;
    let f = |n| float_child(ch, n, "bounds_incomplete", "bounds_type");
    Ok(CartesianBounds {
        x: ordered(f("xMinimum")?, f("xMaximum")?)?,
        y: ordered(f("yMinimum")?, f("yMaximum")?)?,
        z: ordered(f("zMinimum")?, f("zMaximum")?)?,
    })
}

fn spherical(node: &E57Node) -> Result<SphericalBounds, &'static str> {
    let ch = structure(node, "bounds_type")?;
    let f = |n| float_child(ch, n, "bounds_incomplete", "bounds_type");
    Ok(SphericalBounds {
        range: ordered(f("rangeMinimum")?, f("rangeMaximum")?)?,
        elevation: ordered(f("elevationMinimum")?, f("elevationMaximum")?)?,
        azimuth: [f("azimuthStart")?, f("azimuthEnd")?],
    })
}

fn scalar(ch: &[(Key, E57Node)], name: &str) -> Result<Scalar, &'static str> {
    match find(ch, name) {
        Some(E57Node::Integer { value: Some(v), .. }) => Ok(Scalar::Integer(*v)),
        Some(E57Node::ScaledInteger { raw: Some(raw), scale, offset, .. }) => {
            Ok(Scalar::ScaledInteger { raw: *raw, scale: *scale, offset: *offset })
        }
        Some(E57Node::Float { value: Some(v), .. }) => Ok(Scalar::Float(*v)),
        Some(E57Node::Integer { .. } | E57Node::ScaledInteger { .. } | E57Node::Float { .. }) => Err("value_missing"),
        Some(_) => Err("limits_type"),
        None => Err("limits_incomplete"),
    }
}

fn range(ch: &[(Key, E57Node)], lo: &str, hi: &str) -> Result<ScalarRange, &'static str> {
    let (minimum, maximum) = (scalar(ch, lo)?, scalar(ch, hi)?);
    let ok = match (minimum, maximum) {
        (Scalar::Integer(a), Scalar::Integer(b)) => a <= b,
        (Scalar::Float(a), Scalar::Float(b)) => a <= b,
        (
            Scalar::ScaledInteger { raw: a, scale: sa, offset: oa },
            Scalar::ScaledInteger { raw: b, scale: sb, offset: ob },
        ) if sa == sb && oa == ob => a <= b,
        _ => return Err("limits_kind_mismatch"),
    };
    if ok {
        Ok(ScalarRange { minimum, maximum })
    } else {
        Err("limits_inverted")
    }
}

fn field(key: &Key, node: &E57Node) -> Result<FieldDesc, &'static str> {
    let kind = match node {
        E57Node::Integer { minimum, maximum, .. } => FieldKind::Integer { minimum: *minimum, maximum: *maximum },
        E57Node::ScaledInteger { minimum, maximum, scale, offset, .. } => {
            FieldKind::ScaledInteger { minimum: *minimum, maximum: *maximum, scale: *scale, offset: *offset }
        }
        E57Node::Float { precision, minimum, maximum, .. } => {
            FieldKind::Float { precision: *precision, minimum: *minimum, maximum: *maximum }
        }
        E57Node::String(_) => FieldKind::String,
        _ => return Err("prototype_field_kind"),
    };
    Ok(FieldDesc { name: key.clone(), kind })
}

pub(crate) fn scan_desc(node: &E57Node) -> Result<ScanDesc, &'static str> {
    let ch = structure(node, "scan_not_structure")?;
    let guid = match find(ch, "guid") {
        Some(E57Node::String(t)) => t.clone(),
        Some(_) => return Err("scan_string_type"),
        None => return Err("scan_guid_missing"),
    };
    let (points, prototype, codecs) = match find(ch, "points") {
        Some(E57Node::CompressedVector { file_offset, record_count, prototype, codecs }) => {
            let fields = structure(prototype, "prototype_not_structure")?;
            if fields.is_empty() {
                return Err("prototype_empty");
            }
            let prototype = fields.iter().map(|(k, n)| field(k, n)).collect::<Result<Vec<_>, _>>()?;
            let codecs = match codecs.as_deref() {
                None => 0,
                Some(E57Node::Vector { children, .. }) => children.len() as u64,
                Some(_) => return Err("codecs_not_vector"),
            };
            (SectionRef { offset: *file_offset, records: *record_count }, prototype, codecs)
        }
        Some(_) => return Err("scan_points_type"),
        None => return Err("scan_points_missing"),
    };
    let intensity_limits = match find(ch, "intensityLimits") {
        None => None,
        Some(n) => Some(range(structure(n, "limits_type")?, "intensityMinimum", "intensityMaximum")?),
    };
    let color_limits = match find(ch, "colorLimits") {
        None => None,
        Some(n) => {
            let c = structure(n, "limits_type")?;
            Some(ColorLimits {
                red: range(c, "colorRedMinimum", "colorRedMaximum")?,
                green: range(c, "colorGreenMinimum", "colorGreenMaximum")?,
                blue: range(c, "colorBlueMinimum", "colorBlueMaximum")?,
            })
        }
    };
    Ok(ScanDesc {
        guid,
        name: opt_string(ch, "name")?,
        description: opt_string(ch, "description")?,
        pose: find(ch, "pose").map(pose).transpose()?,
        points,
        prototype,
        codecs,
        cartesian_bounds: find(ch, "cartesianBounds").map(cartesian).transpose()?,
        spherical_bounds: find(ch, "sphericalBounds").map(spherical).transpose()?,
        intensity_limits,
        color_limits,
    })
}

// ---- the writer's tree form ----

fn k(local: &str) -> Key {
    Key::new(local)
}

fn float(v: f64) -> E57Node {
    E57Node::Float { value: Some(v), precision: Some(Precision::Double), minimum: None, maximum: None }
}

fn scalar_node(s: Scalar) -> E57Node {
    match s {
        Scalar::Integer(v) => E57Node::Integer { value: Some(v), minimum: None, maximum: None },
        Scalar::ScaledInteger { raw, scale, offset } => {
            E57Node::ScaledInteger { raw: Some(raw), minimum: None, maximum: None, scale, offset }
        }
        Scalar::Float(v) => float(v),
    }
}

fn floats(names: &[&str], values: &[f64]) -> E57Node {
    E57Node::Structure(names.iter().zip(values).map(|(n, v)| (k(n), float(*v))).collect())
}

fn range_nodes(out: &mut Vec<(Key, E57Node)>, lo: &str, hi: &str, r: &ScalarRange) {
    out.push((k(lo), scalar_node(r.minimum)));
    out.push((k(hi), scalar_node(r.maximum)));
}

fn field_node(f: &FieldDesc) -> E57Node {
    match f.kind {
        FieldKind::Integer { minimum, maximum } => E57Node::Integer { value: None, minimum, maximum },
        FieldKind::ScaledInteger { minimum, maximum, scale, offset } => {
            E57Node::ScaledInteger { raw: None, minimum, maximum, scale, offset }
        }
        FieldKind::Float { precision, minimum, maximum } => E57Node::Float { value: None, precision, minimum, maximum },
        FieldKind::String => E57Node::String(XmlText::new("")),
    }
}

impl ScanDesc {
    /// The `data3D` child the writer emits for this scan. Children, in order: `guid`,
    /// `name`, `description`, `pose` (`rotation` `w x y z`, `translation` `x y z`), `points`
    /// (prototype fields with no value, an empty `codecs` vector), `cartesianBounds`,
    /// `sphericalBounds`, `intensityLimits`, `colorLimits`; absent options are left out.
    /// Reals are `Float` elements with `precision="double"`. `codecs` is not written (the
    /// caller refuses a non-zero count).
    pub(crate) fn to_node(&self) -> E57Node {
        let mut ch = vec![(k("guid"), E57Node::String(self.guid.clone()))];
        if let Some(n) = &self.name {
            ch.push((k("name"), E57Node::String(n.clone())));
        }
        if let Some(d) = &self.description {
            ch.push((k("description"), E57Node::String(d.clone())));
        }
        if let Some(p) = &self.pose {
            let pose = E57Node::Structure(vec![
                (k("rotation"), floats(&["w", "x", "y", "z"], &p.q)),
                (k("translation"), floats(&["x", "y", "z"], &p.t)),
            ]);
            ch.push((k("pose"), pose));
        }
        let fields = self.prototype.iter().map(|f| (f.name.clone(), field_node(f))).collect();
        ch.push((
            k("points"),
            E57Node::CompressedVector {
                file_offset: self.points.offset,
                record_count: self.points.records,
                prototype: Box::new(E57Node::Structure(fields)),
                codecs: Some(Box::new(E57Node::Vector { allow_heterogeneous: None, children: Vec::new() })),
            },
        ));
        if let Some(b) = &self.cartesian_bounds {
            let names = ["xMinimum", "xMaximum", "yMinimum", "yMaximum", "zMinimum", "zMaximum"];
            ch.push((k("cartesianBounds"), floats(&names, &[b.x[0], b.x[1], b.y[0], b.y[1], b.z[0], b.z[1]])));
        }
        if let Some(b) = &self.spherical_bounds {
            let names =
                ["rangeMinimum", "rangeMaximum", "elevationMinimum", "elevationMaximum", "azimuthStart", "azimuthEnd"];
            let v = [b.range[0], b.range[1], b.elevation[0], b.elevation[1], b.azimuth[0], b.azimuth[1]];
            ch.push((k("sphericalBounds"), floats(&names, &v)));
        }
        if let Some(r) = &self.intensity_limits {
            let mut l = Vec::new();
            range_nodes(&mut l, "intensityMinimum", "intensityMaximum", r);
            ch.push((k("intensityLimits"), E57Node::Structure(l)));
        }
        if let Some(c) = &self.color_limits {
            let mut l = Vec::new();
            range_nodes(&mut l, "colorRedMinimum", "colorRedMaximum", &c.red);
            range_nodes(&mut l, "colorGreenMinimum", "colorGreenMaximum", &c.green);
            range_nodes(&mut l, "colorBlueMinimum", "colorBlueMaximum", &c.blue);
            ch.push((k("colorLimits"), E57Node::Structure(l)));
        }
        E57Node::Structure(ch)
    }
}
