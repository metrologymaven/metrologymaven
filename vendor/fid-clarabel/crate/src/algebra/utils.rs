// Modified from Clarabel 0.11.1 by Metrology Maven;
// see vendor/fid-clarabel/patches/
//! Internal utility functions and helpers.

// assorted other functionality missing from std

// a drop-in replacement for the julia "findall" function,
// which serves as a vectorized version of the std::iter::position
// returning indices of *all* elements satisfying a predicate

use std::cmp::Ordering;

#[allow(dead_code)]
pub(crate) trait PositionAll<T>: Iterator<Item = T> {
    fn position_all<F>(&mut self, predicate: F) -> Vec<usize>
    where
        F: FnMut(&T) -> bool;
}

impl<T, I> PositionAll<T> for I
where
    I: Iterator<Item = T>,
{
    fn position_all<F>(&mut self, mut f: F) -> Vec<usize>
    where
        F: FnMut(&T) -> bool,
    {
        self.enumerate()
            .filter(|(_, item)| f(item))
            .map(|(index, _)| index)
            .collect::<Vec<_>>()
    }
}

// permutation and inverse permutation
pub(crate) fn permute<T: Copy>(x: &mut [T], b: &[T], p: &[usize]) {
    crate::qdldl::permute(x, b, p);
}

#[allow(dead_code)]
pub(crate) fn ipermute<T: Copy>(x: &mut [T], b: &[T], p: &[usize]) {
    crate::qdldl::ipermute(x, b, p);
}

// Construct an inverse permutation from a permutation
#[allow(dead_code)]
pub(crate) fn invperm(p: &[usize]) -> Vec<usize> {
    let mut b = vec![0; p.len()];
    for (i, j) in p.iter().enumerate() {
        // invariant (patch 0003): p is a permutation built by the solver.
        assert!(*j < p.len() && b[*j] == 0);
        b[*j] = i;
    }
    b
}

#[allow(dead_code)]
pub(crate) fn sortperm<T>(p: &mut [usize], v: &[T])
where
    T: Sized + Ord + Copy,
{
    // invariant (patch 0003): the permutation buffer is allocated with the length of v.
    assert_eq!(p.len(), v.len());
    p.iter_mut().enumerate().for_each(|(i, p)| *p = i);
    p.sort_by_key(|&k| v[k]);
}

#[allow(dead_code)]
pub(crate) fn sortperm_rev<T>(p: &mut [usize], v: &[T])
where
    T: Sized + Ord + Copy,
{
    // invariant (patch 0003): the permutation buffer is allocated with the length of v.
    assert_eq!(p.len(), v.len());
    sortperm_by(p, v, |&a, &b| b.cmp(&a));
}

pub(crate) fn sortperm_by<T, F>(p: &mut [usize], v: &[T], compare: F)
where
    F: FnMut(&T, &T) -> Ordering,
{
    // invariant (patch 0003): the permutation buffer is allocated with the length of v.
    assert_eq!(p.len(), v.len());
    p.iter_mut().enumerate().for_each(|(i, p)| *p = i);
    let mut f = compare;
    p.sort_by(|&i, &j| f(&v[i], &v[j]));
}

// PJG: maybe should be a vector trait, but T needs to admit
// non-float types (e.g. usize).  Would require partition of the
// vector math traits into those that require FloatT and those
// that only require Num + Ord.
#[allow(dead_code)]
pub(crate) fn findmax<T>(v: &[T]) -> Option<usize>
where
    T: Copy + Ord,
{
    v.iter()
        .enumerate()
        .max_by_key(|(_, &value)| value)
        .map(|(idx, _)| idx)
}

// -------------
// testing
