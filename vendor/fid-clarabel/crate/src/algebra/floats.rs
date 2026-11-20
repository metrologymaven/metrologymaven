// Modified from Clarabel 0.11.1 by Metrology Maven;
// see vendor/fid-clarabel/patches/
#![allow(non_snake_case)]
//! The solver's floating point type.
//!
//! Upstream Clarabel is generic over a float type `T: FloatT` built on `num_traits`. This
//! fork is specialised to `f64` (patch 0002): `T` is an alias of `f64`, and [`FloatT`] only
//! supplies the constructors and predicates the solver used from `num_traits`.

/// The solver's floating point type: always `f64` in this fork.
pub type T = f64;

/// Constructors and predicates on the solver's float type ([`T`] = `f64`).
pub trait FloatT: Copy {
    /// `0.0`
    fn zero() -> Self;
    /// `1.0`
    fn one() -> Self;
    /// Machine epsilon.
    fn epsilon() -> Self;
    /// Positive infinity.
    fn infinity() -> Self;
    /// A quiet NaN (used only for "no value" placeholders in the solution).
    fn nan() -> Self;
    /// `self == 0.0`
    fn is_zero(&self) -> bool;
}

impl FloatT for f64 {
    #[inline]
    fn zero() -> Self {
        0.0
    }
    #[inline]
    fn one() -> Self {
        1.0
    }
    #[inline]
    fn epsilon() -> Self {
        f64::EPSILON
    }
    #[inline]
    fn infinity() -> Self {
        f64::INFINITY
    }
    #[inline]
    fn nan() -> Self {
        f64::NAN
    }
    #[inline]
    fn is_zero(&self) -> bool {
        *self == 0.0
    }
}

/// Trait for converting Rust primitives to [`T`].
///
/// Lets the solver write constants as `(2.0).as_T()` as upstream does.
pub(crate) trait AsFloatT: 'static {
    fn as_T(&self) -> T;
}

macro_rules! impl_as_FloatT {
    ($ty:ty) => {
        impl AsFloatT for $ty {
            #[inline]
            fn as_T(&self) -> T {
                *self as T
            }
        }
    };
}
impl_as_FloatT!(u32);
impl_as_FloatT!(u64);
impl_as_FloatT!(usize);
impl_as_FloatT!(f32);
impl_as_FloatT!(f64);
