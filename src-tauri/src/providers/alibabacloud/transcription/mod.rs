mod types;
pub use types::{TranscriptionParams, TranscriptionResult};

fn default_transcription_model() -> String {
    crate::providers::registry::default_file_model().to_string()
}
