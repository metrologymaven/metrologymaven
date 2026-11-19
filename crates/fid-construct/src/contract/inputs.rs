//! Inputs bound to a declaration: the only channel through which a construction reads
//! geometry.
//!
//! [`Inputs::bind`] pairs a [`DependencyDecl`] with the supplied geometry, in declared
//! order, after checking the count and every input. A construction then reads an input only
//! by an [`ElementRef`] the declaration lists (the handle it built its declaration from):
//! asking for any other reference is [`ConstructError::UndeclaredInput`]. Since `evaluate`
//! receives nothing but the bound inputs and the parameters (no store, no project, no
//! clock), a construction cannot read an element it did not declare; the
//! [`super::harness`] additionally detects hidden state kept in the construction value.

use fid_fit::contract::{FittedFeature, Primitive, V3};

use super::decl::DependencyDecl;
use super::error::ConstructError;
use super::geom::{ElementGeom, InputKind, Scalar};
use super::id::ElementRef;
use super::params::QuantityKind;

/// The supplied geometry, bound to the declaration it was supplied for. Fields are private;
/// only [`Inputs::bind`] builds one.
#[derive(Debug, Clone, Copy)]
pub struct Inputs<'a> {
    decl: &'a DependencyDecl,
    geoms: &'a [ElementGeom<'a>],
}

impl<'a> Inputs<'a> {
    /// Binds `geoms` (declared order) to `decl`. [`ConstructError::InputCount`] when the
    /// counts differ; [`ConstructError::InvalidFeature`] (with the index) for the first
    /// invalid feature.
    pub fn bind(decl: &'a DependencyDecl, geoms: &'a [ElementGeom<'a>]) -> Result<Inputs<'a>, ConstructError> {
        let expected = decl.inputs().len();
        if geoms.len() != expected {
            return Err(ConstructError::InputCount { expected, got: geoms.len() });
        }
        for (i, g) in geoms.iter().enumerate() {
            g.validate(i)?;
        }
        Ok(Inputs { decl, geoms })
    }

    /// The declaration the inputs are bound to.
    pub fn decl(&self) -> &'a DependencyDecl {
        self.decl
    }

    /// Number of inputs.
    pub fn len(&self) -> usize {
        self.geoms.len()
    }

    /// Whether there is no input.
    pub fn is_empty(&self) -> bool {
        self.geoms.is_empty()
    }

    /// The geometry declared as `r`, with its declared index.
    /// [`ConstructError::UndeclaredInput`] when the declaration does not list `r` (element
    /// and role must both match).
    pub fn get(&self, r: &ElementRef) -> Result<(usize, &'a ElementGeom<'a>), ConstructError> {
        let at = self.decl.inputs().iter().position(|d| d == r);
        match at.and_then(|i| self.geoms.get(i).map(|g| (i, g))) {
            Some(found) => Ok(found),
            None => Err(ConstructError::UndeclaredInput { role: r.role() }),
        }
    }

    /// Every input with its reference, in declared order (for variadic constructions such
    /// as a best fit through feature centres).
    pub fn iter(&self) -> impl Iterator<Item = (ElementRef, &'a ElementGeom<'a>)> + 'a {
        self.decl.inputs().iter().copied().zip(self.geoms.iter())
    }

    /// The feature declared as `r`, which must be of kind `expected` (or any feature).
    /// [`ConstructError::WrongInputKind`] otherwise.
    pub fn feature(&self, r: &ElementRef, expected: InputKind) -> Result<&'a FittedFeature, ConstructError> {
        let (index, g) = self.get(r)?;
        match g {
            ElementGeom::Feature(f) if expected.accepts(g.kind()) => Ok(f),
            _ => Err(ConstructError::WrongInputKind { index, expected, got: g.kind() }),
        }
    }

    /// The position of the point feature declared as `r`.
    pub fn point(&self, r: &ElementRef) -> Result<V3, ConstructError> {
        match self.feature(r, InputKind::Feature(Primitive::Point))? {
            FittedFeature::Point { c } => Ok(*c),
            other => Err(ConstructError::WrongInputKind {
                index: self.get(r)?.0,
                expected: InputKind::Feature(Primitive::Point),
                got: InputKind::Feature(other.primitive()),
            }),
        }
    }

    /// The scalar declared as `r`, which must have kind `kind`.
    pub fn scalar(&self, r: &ElementRef, kind: QuantityKind) -> Result<Scalar, ConstructError> {
        let (index, g) = self.get(r)?;
        let expected = InputKind::Scalar(kind);
        match g {
            ElementGeom::Scalar(s) if s.kind() == kind => Ok(*s),
            _ => Err(ConstructError::WrongInputKind { index, expected, got: g.kind() }),
        }
    }
}
