//! Semantic selection query language v1: closed JSON AST grammar, bounded iterative parser,
//! canonical form and SHA-256 digest (spec node `GEO.SEL.QUERY.GRAMMAR`).

pub mod ast;
pub mod canon;
pub mod eval;
pub mod parse;

pub use ast::{
    format_entity_id, parse_entity_id, AttrChannel, Expr, NearRef, Predicate, QueryDigest, QueryError, SelectionQuery,
    TopologyId, MAX_AST_NODES, MAX_IDS_PER_LIST, MAX_QUERY_BYTES, MAX_QUERY_DEPTH, MAX_TOPOLOGY_ID_BYTES,
    QUERY_VERSION,
};
pub use canon::{
    canonicalize, canonicalize_expr, digest_canonical_bytes, format_jcs_number, parse_and_canonicalize, query_digest,
    to_canonical_bytes, to_opaque_query,
};
pub use eval::{evaluate, evaluate_selection, CadEvaluator, CadPred, EvalContext, EvalError, NearEvaluator, NearPred};
pub use parse::{parse_opaque_query, parse_query};
