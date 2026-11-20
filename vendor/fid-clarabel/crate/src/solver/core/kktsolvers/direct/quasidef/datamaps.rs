// Modified from Clarabel 0.11.1 by Metrology Maven;
// see vendor/fid-clarabel/patches/
#![allow(non_snake_case)]

use super::*;
use crate::solver::core::cones::*;
// KNP-1 (patch 0007): hand-written dispatch below; upstream used enum_dispatch.
pub(crate) enum SparseExpansionMap {
    SOCExpansionMap(SOCExpansionMap),
}

pub(crate) enum SparseExpansionCone<'a> {
    SecondOrderCone(&'a SecondOrderCone),
}

impl SupportedCone {
    pub(crate) fn to_sparse_expansion(&self) -> Option<SparseExpansionCone<'_>> {
        match self {
            SupportedCone::SecondOrderCone(sc) => Some(SparseExpansionCone::SecondOrderCone(sc)),
            _ => None,
        }
    }
}

pub(crate) trait SparseExpansionMapTrait {
    fn pdim(&self) -> usize;
    fn nnz_vec(&self) -> usize;
    fn Dsigns(&self) -> &[i8];
}

impl SparseExpansionMapTrait for Vec<SparseExpansionMap> {
    fn pdim(&self) -> usize {
        self.iter().fold(0, |pdim, map| pdim + map.pdim())
    }
    fn nnz_vec(&self) -> usize {
        self.iter().fold(0, |nnz, map| nnz + map.nnz_vec())
    }
    fn Dsigns(&self) -> &[i8] {
        // not meaningful for a collection of maps (upstream: unreachable!())
        &[]
    }
}

type UpdateFcn = fn(&mut BoxedDirectLDLSolver, &mut CscMatrix, &[usize], &[T]) -> ();
type ScaleFcn = fn(&mut BoxedDirectLDLSolver, &mut CscMatrix, &[usize], T) -> ();

pub(crate) trait SparseExpansionConeTrait {
    fn expansion_map(&self) -> SparseExpansionMap;
    fn csc_colcount_sparsecone(
        &self,
        map: &SparseExpansionMap,
        K: &mut CscMatrix,
        row: usize,
        col: usize,
        shape: MatrixTriangle,
    );
    fn csc_fill_sparsecone(
        &self,
        map: &mut SparseExpansionMap,
        K: &mut CscMatrix,
        row: usize,
        col: usize,
        shape: MatrixTriangle,
    );
    fn csc_update_sparsecone(
        &self,
        map: &SparseExpansionMap,
        ldl: &mut BoxedDirectLDLSolver,
        K: &mut CscMatrix,
        updateFcn: UpdateFcn,
        scaleFcn: ScaleFcn,
    );
}

//--------------------------------------
// Second order cone data map
//--------------------------------------

pub(crate) struct SOCExpansionMap {
    pub(crate) u: Vec<usize>, //off diag dense columns u
    pub(crate) v: Vec<usize>, //off diag dense columns v
    pub(crate) D: [usize; 2], //diag D
}

impl SOCExpansionMap {
    pub fn new(cone: &SecondOrderCone) -> Self {
        let u = vec![0; cone.numel()];
        let v = vec![0; cone.numel()];
        let D = [0; 2];
        Self { u, v, D }
    }
}

impl SparseExpansionMapTrait for SOCExpansionMap {
    fn pdim(&self) -> usize {
        2
    }
    fn nnz_vec(&self) -> usize {
        2 * self.v.len()
    }
    fn Dsigns(&self) -> &[i8] {
        &[-1, 1]
    }
}

// (upstream generated this with the impl_map_recover! macro)
impl<'a> SecondOrderCone {
    pub(crate) fn recover_map(&self, map: &'a SparseExpansionMap) -> &'a SOCExpansionMap {
        match map {
            SparseExpansionMap::SOCExpansionMap(map) => map,
        }
    }
    pub(crate) fn recover_map_mut(
        &self,
        map: &'a mut SparseExpansionMap,
    ) -> &'a mut SOCExpansionMap {
        match map {
            SparseExpansionMap::SOCExpansionMap(map) => map,
        }
    }
}

impl SparseExpansionConeTrait for &'_ SecondOrderCone {
    fn expansion_map(&self) -> SparseExpansionMap {
        SparseExpansionMap::SOCExpansionMap(SOCExpansionMap::new(self))
    }

    fn csc_colcount_sparsecone(
        &self,
        map: &SparseExpansionMap,
        K: &mut CscMatrix,
        row: usize,
        col: usize,
        shape: MatrixTriangle,
    ) {
        let map = self.recover_map(map);
        let nvars = self.numel();

        match shape {
            MatrixTriangle::Triu => {
                K.colcount_colvec(nvars, row, col); // u column
                K.colcount_colvec(nvars, row, col + 1); // v column
            }
            MatrixTriangle::Tril => {
                K.colcount_rowvec(nvars, col, row); // u row
                K.colcount_rowvec(nvars, col + 1, row); // v row
            }
        }
        K.colcount_diag(col, map.pdim());
    }

    fn csc_fill_sparsecone(
        &self,
        map: &mut SparseExpansionMap,
        K: &mut CscMatrix,
        row: usize,
        col: usize,
        shape: MatrixTriangle,
    ) {
        let map = self.recover_map_mut(map);

        // fill structural zeros for u and v columns for this cone
        // note v is the first extra row/column, u is second
        match shape {
            MatrixTriangle::Triu => {
                K.fill_colvec(&mut map.v, row, col); //u
                K.fill_colvec(&mut map.u, row, col + 1); //v
            }
            MatrixTriangle::Tril => {
                K.fill_rowvec(&mut map.v, col, row); //u
                K.fill_rowvec(&mut map.u, col + 1, row); //v
            }
        }
        let pdim = map.pdim();
        K.fill_diag(&mut map.D, col, pdim);
    }

    fn csc_update_sparsecone(
        &self,
        map: &SparseExpansionMap,
        ldl: &mut BoxedDirectLDLSolver,
        K: &mut CscMatrix,
        updateFcn: UpdateFcn,
        scaleFcn: ScaleFcn,
    ) {
        let Some(sparse_data) = self.sparse_data.as_ref() else {
            return;
        };

        let map = self.recover_map(map);
        let eta2 = self.eta * self.eta;

        // off diagonal columns (or rows)
        updateFcn(ldl, K, &map.u, &sparse_data.u);
        updateFcn(ldl, K, &map.v, &sparse_data.v);
        scaleFcn(ldl, K, &map.u, -eta2);
        scaleFcn(ldl, K, &map.v, -eta2);

        //set diagonal to η^2*(-1,1) in the extended rows/cols
        updateFcn(ldl, K, &map.D, &[-eta2, eta2]);
    }
}

//--------------------------------------
// LDL Data Map
//--------------------------------------

pub(crate) struct LDLDataMap {
    pub P: Vec<usize>,
    pub A: Vec<usize>,
    pub Hsblocks: Vec<usize>, //indices of the lower RHS blocks (by cone)
    pub sparse_maps: Vec<SparseExpansionMap>, //sparse cone expansion terms

    // all of above terms should be disjoint and their union
    // should cover all of the user data in the KKT matrix.  Now
    // we make two last redundant indices that will tell us where
    // the whole diagonal is, including structural zeros.
    pub diagP: Vec<usize>,
    pub diag_full: Vec<usize>,
}

impl LDLDataMap {
    pub fn new(Pmat: &CscMatrix, Amat: &CscMatrix, cones: &CompositeCone) -> Self {
        let (m, n) = (Amat.nrows(), Pmat.nrows());
        let P = vec![0; Pmat.nnz()];
        let A = vec![0; Amat.nnz()];

        // the diagonal of the ULHS KKT block P.
        // NB : we fill in structural zeros here even if the matrix
        // P is empty (e.g. as in an LP), so we can have entries in
        // index Pdiag that are not present in the index P
        let diagP = vec![0; n];

        // make an index for each of the Hs blocks for each cone
        let Hsblocks = allocate_kkt_Hsblocks::<usize>(cones);

        // now do the sparse cone expansion pieces
        let nsparse = cones.iter().filter(|&c| c.is_sparse_expandable()).count();
        let mut sparse_maps = Vec::with_capacity(nsparse);

        for cone in cones.iter() {
            if cone.is_sparse_expandable() {
                if let Some(sc) = cone.to_sparse_expansion() {
                    sparse_maps.push(sc.expansion_map());
                }
            }
        }

        let diag_full = vec![0; m + n + sparse_maps.pdim()];

        Self {
            P,
            A,
            Hsblocks,
            sparse_maps,
            diagP,
            diag_full,
        }
    }
}

impl SparseExpansionMapTrait for SparseExpansionMap {
    fn pdim(&self) -> usize {
        match self {
            SparseExpansionMap::SOCExpansionMap(m) => m.pdim(),
        }
    }
    fn nnz_vec(&self) -> usize {
        match self {
            SparseExpansionMap::SOCExpansionMap(m) => m.nnz_vec(),
        }
    }
    fn Dsigns(&self) -> &[i8] {
        match self {
            SparseExpansionMap::SOCExpansionMap(m) => m.Dsigns(),
        }
    }
}

impl SparseExpansionConeTrait for SparseExpansionCone<'_> {
    fn expansion_map(&self) -> SparseExpansionMap {
        match self {
            SparseExpansionCone::SecondOrderCone(c) => c.expansion_map(),
        }
    }
    fn csc_colcount_sparsecone(
        &self,
        map: &SparseExpansionMap,
        K: &mut CscMatrix,
        row: usize,
        col: usize,
        shape: MatrixTriangle,
    ) {
        match self {
            SparseExpansionCone::SecondOrderCone(c) => {
                c.csc_colcount_sparsecone(map, K, row, col, shape)
            }
        }
    }
    fn csc_fill_sparsecone(
        &self,
        map: &mut SparseExpansionMap,
        K: &mut CscMatrix,
        row: usize,
        col: usize,
        shape: MatrixTriangle,
    ) {
        match self {
            SparseExpansionCone::SecondOrderCone(c) => {
                c.csc_fill_sparsecone(map, K, row, col, shape)
            }
        }
    }
    fn csc_update_sparsecone(
        &self,
        map: &SparseExpansionMap,
        ldl: &mut BoxedDirectLDLSolver,
        K: &mut CscMatrix,
        updateFcn: UpdateFcn,
        scaleFcn: ScaleFcn,
    ) {
        match self {
            SparseExpansionCone::SecondOrderCone(c) => {
                c.csc_update_sparsecone(map, ldl, K, updateFcn, scaleFcn)
            }
        }
    }
}
