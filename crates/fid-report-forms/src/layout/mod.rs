//! Fixed form templates, field maps, `form_field` block kind & resolver, locking, clearance
//! release gate and clean-room label checks (`RPT.FORMS.LAYOUT`).

pub mod fai;
pub mod map;

pub use map::{
    check_cleanroom_labels, check_release_clearance, command_defs, current_build_profile, resolve_form_field_block,
    resolve_template_fields, BuildProfile, Citation, CleanroomPhraseSet, ClearanceCheckReport, FieldEntry, FieldMap,
    FormClearance, FormFieldKind, FormFieldPayload, FormFieldResolver, FormKind, FormLayoutInfo, FormLayoutRegistry,
    FormNumericFormatter, FormTemplateEntry, LayoutError, ResolvedFormField, ResolvedFormFieldValue, SourcePath,
    FIELD_MAP_SCHEMA_VERSION, FORM_FIELD_KIND, MAX_CITATION_BYTES, MAX_FIELD_MAP_ENTRIES, SHOW_LAYOUT_FAI,
    SHOW_LAYOUT_PPAP,
};
