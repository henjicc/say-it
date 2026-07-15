//! OCR error type definitions

use thiserror::Error;

use crate::mnn::MnnError;

/// OCR error type
#[derive(Error, Debug)]
pub enum OcrError {
    /// MNN inference engine error
    #[error("MNN inference error: {0}")]
    MnnError(#[from] MnnError),

    /// Image processing error
    #[error("Image processing error: {0}")]
    ImageError(#[from] image::ImageError),

    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Invalid parameter error
    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    /// Model loading error
    #[error("Model loading failed: {0}")]
    ModelLoadError(String),

    /// Preprocessing error
    #[error("Preprocessing error: {0}")]
    PreprocessError(String),

    /// Postprocessing error
    #[error("Postprocessing error: {0}")]
    PostprocessError(String),

    /// Detection error
    #[error("Detection error: {0}")]
    DetectionError(String),

    /// Recognition error
    #[error("Recognition error: {0}")]
    RecognitionError(String),

    /// Not initialized error
    #[error("Not initialized: {0}")]
    NotInitialized(String),

    /// Charset parsing error
    #[error("Charset parsing error: {0}")]
    CharsetError(String),
}

/// OCR result type alias
pub type OcrResult<T> = std::result::Result<T, OcrError>;
