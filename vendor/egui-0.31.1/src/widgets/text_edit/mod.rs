// Cedar receipt prototype, 2026-10-15: observational additions to egui 0.31.1.
mod builder;
mod output;
mod state;
mod text_buffer;

pub use {
    crate::text_selection::TextCursorState,
    builder::TextEdit,
    output::{ImeEventOutcome, TextEditImeReceipt, TextEditOutput},
    state::TextEditState,
    text_buffer::TextBuffer,
};
