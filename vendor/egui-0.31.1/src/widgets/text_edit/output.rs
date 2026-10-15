// Cedar receipt prototype, 2026-10-15: observational additions to egui 0.31.1.
use std::sync::Arc;

use crate::text::CursorRange;

/// The output from a [`TextEdit`](crate::TextEdit).
pub struct TextEditOutput {
    /// The interaction response.
    pub response: crate::Response,

    /// How the text was displayed.
    pub galley: Arc<crate::Galley>,

    /// Where the text in [`Self::galley`] ended up on the screen.
    pub galley_pos: crate::Pos2,

    /// The text was clipped to this rectangle when painted.
    pub text_clip_rect: crate::Rect,

    /// The state we stored after the run.
    pub state: super::TextEditState,

    /// IME branches actually reached by this invocation, in native processing order.
    /// Empty means no branch was observed, not that a Commit was rejected.
    /// This is transient: it is never stored in `TextEditState` or replayed.
    pub ime_receipts: Vec<TextEditImeReceipt>,

    /// Where the text cursor is.
    pub cursor_range: Option<CursorRange>,
}

impl TextEditOutput {
    #[deprecated = "Renamed `self.galley_pos`"]
    pub fn text_draw_pos(&self) -> crate::Pos2 {
        self.galley_pos
    }
}

// TODO(emilk): add `output.paint` and `output.store` and split out that code from `TextEdit::show`.

/// A transient observation of an actual native IME branch, without payload text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextEditImeReceipt {
    /// Index in this TextEdit's InputState.events snapshot, before filtering/sorting.
    /// Other widgets may have removed events already; this is NOT a RawInput index.
    pub input_event_index: usize,
    /// Position in the actual filtered/sorted event loop, including non-IME events.
    pub processing_order: usize,
    /// The branch actually executed, not inferred from the final buffer or cursor.
    pub outcome: ImeEventOutcome,
}

/// Observed native IME branch. Acceptance does not imply text changed or full insertion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImeEventOutcome {
    /// Native IME state enabled and its cursor saved.
    Enabled,
    /// Native Preedit replacement branch ran (possibly empty).
    PreeditApplied,
    /// Native Preedit ignored a standalone CR or LF.
    PreeditIgnoredLineBreak,
    /// The actual nonempty Commit insertion branch ran, even if truncated to zero.
    CommitAccepted {
        /// Character index after deletion, before the actual insertion.
        insertion_start: usize,
        /// Cursor character index after the actual insertion helper returned.
        /// A custom TextBuffer may define its own cursor-update behavior.
        insertion_end: usize,
    },
    /// Empty Commit collapsed selection but did not insert.
    CommitEmpty,
    /// Native Commit ignored a standalone CR or LF.
    CommitIgnoredLineBreak,
    /// Nonempty Commit failed the native secondary-anchor check.
    CommitRejectedAnchor,
    /// Native IME state disabled.
    Disabled,
}
