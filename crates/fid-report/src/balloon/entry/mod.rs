//! Manual gauge reading entry, voiding, listing and multi-part series entry
//! (`RPT.BALLOON.ENTRY`).

pub mod record;
pub mod series;

pub use record::{
    command_defs, replay_exact, validate_single_reading, AuthoredQuantity, DirectReadingEvaluator, EnterReadingParams,
    EnterReadingResult, EntryError, EntryReplayRun, EntryTimestamp, EntryWarning, ManualReading, ManualReadingEvent,
    ManualReadingStore, PrincipalId, ReadingFlag, ReadingId, ResultRecordRef, StageId, ValidatedSingleReading,
    VoidInfo, VoidReadingParams, ENTER_READING, LIST_READING, MAX_NOTE_BYTES, MAX_VALUE_BYTES, MAX_VOID_REASON_BYTES,
    VOID_READING, WARN_GAUGE_CALIBRATION_OVERDUE, WARN_READING_EXCEEDS_RESOLUTION,
};
pub use series::{
    enter_series_command_def, enter_series_reading, EnterSeriesParams, EnterSeriesResult, RunStageLookup, SeriesError,
    ENTER_SERIES_READING, MAX_SERIES_ELEMENTS,
};
