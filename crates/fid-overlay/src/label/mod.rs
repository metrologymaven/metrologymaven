//! Labels, leader lines and label layout (spec node `VIZ.OVERLAY.LABEL`).
//!
//! - [`model`] (`VIZ.OVERLAY.LABEL.MODEL`): `LabelSet` and `Label` entities, templates,
//!   per-view placements, pinning, canonical digest and the three W2 label commands
//!   (`view.add_label@1`, `view.edit_label@1`, `view.remove_label@1`).

pub mod model;
