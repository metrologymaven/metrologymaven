//! Frame identifiers, frame edges and the frame-graph check (REQ-HAL.IDL.UNITS-005).

use super::{boolean, kept, obj, object, reference, string, take, UnitsError};
use fid_canon::CValue;
use fid_uuid::{uuid_v5, Uuid};
use std::collections::BTreeMap;
use std::fmt;

/// Most frames one graph may hold (parsers are bounded, CONSTITUTION §8).
pub const MAX_FRAMES: usize = 4096;
/// Most edges one graph may hold.
pub const MAX_EDGES: usize = 4096;
/// Longest transform or pose reference, in bytes.
pub const MAX_REF_BYTES: usize = 256;

/// A frame in the session frame graph: a UUIDv5 under the session namespace
/// (CONSTITUTION §5, derived IDs). Wire form: the lowercase hyphenated UUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FrameId(Uuid);

impl FrameId {
    /// The frame ID of `name` in a session: UUIDv5 (RFC 9562 §5.5) of the session namespace
    /// and the UTF-8 bytes of the name.
    pub fn derive(session_namespace: &Uuid, name: &str) -> FrameId {
        FrameId(uuid_v5(session_namespace, name.as_bytes()))
    }

    /// Wraps a UUID; it must be version 5 with the RFC 9562 variant, else `bad_value`.
    pub fn from_uuid(id: Uuid) -> Result<FrameId, UnitsError> {
        if id.version() != 5 || id.variant_bits() != 0b10 {
            return Err(UnitsError::BadValue { field: "frame_id", value: id.to_string() });
        }
        Ok(FrameId(id))
    }

    /// Parses the lowercase hyphenated form of a UUIDv5.
    pub fn parse(text: &str) -> Result<FrameId, UnitsError> {
        let bad = || UnitsError::BadValue { field: "frame_id", value: kept(text) };
        let id = Uuid::parse(text).map_err(|_| bad())?;
        if id.to_string() != text {
            return Err(bad());
        }
        FrameId::from_uuid(id)
    }

    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    fn read(field: &'static str, v: CValue) -> Result<FrameId, UnitsError> {
        FrameId::parse(&string(field, v)?)
    }

    fn to_value(self) -> CValue {
        CValue::Str(self.0.to_string())
    }
}

impl fmt::Display for FrameId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// What a frame is attached to. The usual chain is sensor → mount → device → machine →
/// part, but the kinds put no constraint on the edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FrameKind {
    Sensor,
    Mount,
    Device,
    Machine,
    Part,
}

impl FrameKind {
    /// Every kind, in declaration order.
    pub const ALL: [FrameKind; 5] =
        [FrameKind::Sensor, FrameKind::Mount, FrameKind::Device, FrameKind::Machine, FrameKind::Part];

    /// The wire string, e.g. `sensor`.
    pub fn as_str(self) -> &'static str {
        match self {
            FrameKind::Sensor => "sensor",
            FrameKind::Mount => "mount",
            FrameKind::Device => "device",
            FrameKind::Machine => "machine",
            FrameKind::Part => "part",
        }
    }

    /// Parses the wire string; anything else is `bad_value`.
    pub fn parse(s: &str) -> Result<FrameKind, UnitsError> {
        FrameKind::ALL
            .into_iter()
            .find(|k| k.as_str() == s)
            .ok_or_else(|| UnitsError::BadValue { field: "kind", value: kept(s) })
    }
}

impl fmt::Display for FrameKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A frame declared in a graph. Wire form: `{"id": "…", "kind": "sensor"}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Frame {
    pub id: FrameId,
    pub kind: FrameKind,
}

impl Frame {
    /// The wire fields.
    pub const FIELDS: [&'static str; 2] = ["id", "kind"];

    pub fn to_value(self) -> CValue {
        obj([("id", self.id.to_value()), ("kind", CValue::Str(self.kind.as_str().to_string()))])
    }

    pub fn from_value(v: CValue) -> Result<Frame, UnitsError> {
        let mut o = object(v, "frame", &Frame::FIELDS)?;
        let id = FrameId::read("id", take(&mut o, "id")?)?;
        let kind = FrameKind::parse(&string("kind", take(&mut o, "kind")?)?)?;
        Ok(Frame { id, kind })
    }
}

/// One edge of the frame graph: `child` is located in `parent` by the transform that
/// `transform` names. A `time_varying` edge (an articulated arm joint, a moving axis) may name
/// the pose stream in `pose`. References are opaque here (non-empty, at most
/// [`MAX_REF_BYTES`], no control characters); `GEO` and `ALN` own what they point to.
///
/// Wire form: `{"child": "…", "parent": "…", "pose": "…", "time_varying": false,
/// "transform": "…"}` with `pose` omitted when absent.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FrameEdge {
    pub parent: FrameId,
    pub child: FrameId,
    pub transform: String,
    pub time_varying: bool,
    pub pose: Option<String>,
}

impl FrameEdge {
    /// The wire fields.
    pub const FIELDS: [&'static str; 5] = ["child", "parent", "pose", "time_varying", "transform"];

    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("child".to_string(), self.child.to_value());
        o.insert("parent".to_string(), self.parent.to_value());
        if let Some(p) = &self.pose {
            o.insert("pose".to_string(), CValue::Str(p.clone()));
        }
        o.insert("time_varying".to_string(), CValue::Bool(self.time_varying));
        o.insert("transform".to_string(), CValue::Str(self.transform.clone()));
        CValue::Obj(o)
    }

    pub fn from_value(v: CValue) -> Result<FrameEdge, UnitsError> {
        let mut o = object(v, "edge", &FrameEdge::FIELDS)?;
        let parent = FrameId::read("parent", take(&mut o, "parent")?)?;
        let child = FrameId::read("child", take(&mut o, "child")?)?;
        let transform = reference("transform", take(&mut o, "transform")?, MAX_REF_BYTES)?;
        let time_varying = boolean("time_varying", take(&mut o, "time_varying")?)?;
        let pose = o.remove("pose").map(|p| reference("pose", p, MAX_REF_BYTES)).transpose()?;
        Ok(FrameEdge { parent, child, transform, time_varying, pose })
    }
}

/// The session frame graph: declared frames and the edges between them. Reading checks the
/// shape only; [`FrameGraph::validate`] checks the graph. Wire form:
/// `{"edges": [...], "frames": [...]}`, order kept as given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameGraph {
    pub frames: Vec<Frame>,
    pub edges: Vec<FrameEdge>,
}

/// DFS colours.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    OnPath,
    Done,
}

impl FrameGraph {
    /// The wire fields.
    pub const FIELDS: [&'static str; 2] = ["edges", "frames"];

    pub fn to_value(&self) -> CValue {
        obj([
            ("edges", CValue::Arr(self.edges.iter().map(FrameEdge::to_value).collect())),
            ("frames", CValue::Arr(self.frames.iter().map(|f| f.to_value()).collect())),
        ])
    }

    pub fn from_value(v: CValue) -> Result<FrameGraph, UnitsError> {
        let mut o = object(v, "frame_graph", &FrameGraph::FIELDS)?;
        let frames = array("frames", take(&mut o, "frames")?, MAX_FRAMES)?;
        let edges = array("edges", take(&mut o, "edges")?, MAX_EDGES)?;
        Ok(FrameGraph {
            frames: frames.into_iter().map(Frame::from_value).collect::<Result<_, _>>()?,
            edges: edges.into_iter().map(FrameEdge::from_value).collect::<Result<_, _>>()?,
        })
    }

    /// Checks the graph: frame IDs unique, every edge between declared frames, no frame with
    /// two parents (a repeated edge counts), and no cycle (a self-edge counts). Fails with
    /// `frame_graph_invalid`. Pure and order-independent: shuffling frames or edges never
    /// changes the outcome or the message. The cycle check is an iterative depth-first walk
    /// over a `BTreeMap` adjacency list.
    pub fn validate(&self) -> Result<(), UnitsError> {
        let invalid = |why: String| Err(UnitsError::FrameGraphInvalid(why));
        if self.frames.len() > MAX_FRAMES || self.edges.len() > MAX_EDGES {
            return invalid(format!("more than {MAX_FRAMES} frames or {MAX_EDGES} edges"));
        }
        let mut ids: Vec<FrameId> = self.frames.iter().map(|f| f.id).collect();
        ids.sort();
        if let Some(w) = ids.windows(2).find(|w| w[0] == w[1]) {
            return invalid(format!("frame {} is declared twice", w[0]));
        }
        let mut edges: Vec<&FrameEdge> = self.edges.iter().collect();
        edges.sort();
        let mut parent_of: BTreeMap<FrameId, FrameId> = BTreeMap::new();
        let mut children: BTreeMap<FrameId, Vec<FrameId>> = BTreeMap::new();
        for e in &edges {
            for end in [e.parent, e.child] {
                if ids.binary_search(&end).is_err() {
                    return invalid(format!("edge names undeclared frame {end}"));
                }
            }
            if e.parent == e.child {
                return invalid(format!("frame {} is its own parent (cycle)", e.child));
            }
            if parent_of.insert(e.child, e.parent).is_some() {
                return invalid(format!("frame {} has two parents", e.child));
            }
            children.entry(e.parent).or_default().push(e.child);
        }
        let mut mark: BTreeMap<FrameId, Mark> = BTreeMap::new();
        for &start in &ids {
            if mark.contains_key(&start) {
                continue;
            }
            mark.insert(start, Mark::OnPath);
            let mut stack: Vec<(FrameId, usize)> = vec![(start, 0)];
            while let Some(&(node, next)) = stack.last() {
                match children.get(&node).and_then(|c| c.get(next)).copied() {
                    Some(child) => {
                        if let Some(top) = stack.last_mut() {
                            top.1 = next + 1;
                        }
                        match mark.get(&child) {
                            Some(Mark::OnPath) => return invalid(format!("cycle through frame {child}")),
                            Some(Mark::Done) => {}
                            None => {
                                mark.insert(child, Mark::OnPath);
                                stack.push((child, 0));
                            }
                        }
                    }
                    None => {
                        mark.insert(node, Mark::Done);
                        stack.pop();
                    }
                }
            }
        }
        Ok(())
    }
}

fn array(field: &'static str, v: CValue, max: usize) -> Result<Vec<CValue>, UnitsError> {
    match v {
        CValue::Arr(items) if items.len() <= max => Ok(items),
        CValue::Arr(items) => Err(UnitsError::BadValue { field, value: format!("{} items (max {max})", items.len()) }),
        _ => Err(UnitsError::BadType { field: field.to_string(), expected: "an array" }),
    }
}
