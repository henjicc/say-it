//! Textline orientation classification model
//!
//! Provides textline orientation classification based on PP-LCNet_x1_0_textline_ori

use image::{DynamicImage, GenericImageView};
use ndarray::{Array4, ArrayD};
use std::path::Path;

use crate::error::{OcrError, OcrResult};
use crate::mnn::{InferenceConfig, InferenceEngine};
use crate::preprocess::NormalizeParams;

/// Orientation preprocessing mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriPreprocessMode {
    /// Document orientation (PP-LCNet_x1_0_doc_ori)
    Doc,
    /// Textline orientation (PP-LCNet_x1_0_textline_ori)
    Textline,
}

/// Orientation classification result
#[derive(Debug, Clone)]
pub struct OrientationResult {
    /// Predicted class index
    pub class_idx: usize,
    /// Predicted angle in degrees (best effort mapping)
    pub angle: i32,
    /// Confidence score (softmax probability)
    pub confidence: f32,
    /// Scores for each class (softmax probabilities)
    pub scores: Vec<f32>,
}

impl OrientationResult {
    /// Create new orientation result
    pub fn new(class_idx: usize, angle: i32, confidence: f32, scores: Vec<f32>) -> Self {
        Self {
            class_idx,
            angle,
            confidence,
            scores,
        }
    }

    /// Check if result is valid by confidence threshold
    pub fn is_valid(&self, threshold: f32) -> bool {
        self.confidence >= threshold
    }
}

/// Orientation model options
#[derive(Debug, Clone)]
pub struct OriOptions {
    /// Target input height
    pub target_height: u32,
    /// Target input width
    pub target_width: u32,
    /// Minimum confidence threshold (for caller-side filtering)
    pub min_score: f32,
    /// Shorter side resize for document mode
    pub resize_shorter: u32,
    /// Preprocess mode
    pub preprocess_mode: OriPreprocessMode,
    /// Class index to angle mapping
    pub class_angles: Vec<i32>,
}

impl Default for OriOptions {
    fn default() -> Self {
        Self {
            target_height: 224,
            target_width: 224,
            min_score: 0.5,
            resize_shorter: 256,
            preprocess_mode: OriPreprocessMode::Doc,
            class_angles: vec![0, 90, 180, 270],
        }
    }
}

impl OriOptions {
    /// Create new options
    pub fn new() -> Self {
        Self::default()
    }

    /// Preset for document orientation models
    pub fn doc() -> Self {
        Self::default()
    }

    /// Preset for textline orientation models
    pub fn textline() -> Self {
        Self {
            target_height: 48,
            target_width: 192,
            min_score: 0.5,
            resize_shorter: 256,
            preprocess_mode: OriPreprocessMode::Textline,
            class_angles: vec![0, 180],
        }
    }

    /// Set target height
    pub fn with_target_height(mut self, height: u32) -> Self {
        self.target_height = height;
        self
    }

    /// Set target width
    pub fn with_target_width(mut self, width: u32) -> Self {
        self.target_width = width;
        self
    }

    /// Set minimum confidence threshold
    pub fn with_min_score(mut self, score: f32) -> Self {
        self.min_score = score;
        self
    }

    /// Set resize shorter side (document mode)
    pub fn with_resize_shorter(mut self, size: u32) -> Self {
        self.resize_shorter = size;
        self
    }

    /// Set preprocess mode
    pub fn with_preprocess_mode(mut self, mode: OriPreprocessMode) -> Self {
        self.preprocess_mode = mode;
        self
    }

    /// Set class index to angle mapping
    pub fn with_class_angles(mut self, angles: Vec<i32>) -> Self {
        self.class_angles = angles;
        self
    }
}

/// Textline orientation classification model
pub struct OriModel {
    engine: InferenceEngine,
    options: OriOptions,
    normalize_params: NormalizeParams,
}

impl OriModel {
    /// Create orientation classifier from model file
    pub fn from_file(
        model_path: impl AsRef<Path>,
        config: Option<InferenceConfig>,
    ) -> OcrResult<Self> {
        let engine = InferenceEngine::from_file(model_path, config)?;
        let options = OriOptions::default();
        let mode = options.preprocess_mode;
        Ok(Self {
            engine,
            options,
            normalize_params: normalize_params_for_mode(mode),
        })
    }

    /// Create orientation classifier from model bytes
    pub fn from_bytes(model_bytes: &[u8], config: Option<InferenceConfig>) -> OcrResult<Self> {
        let engine = InferenceEngine::from_buffer(model_bytes, config)?;
        let options = OriOptions::default();
        let mode = options.preprocess_mode;
        Ok(Self {
            engine,
            options,
            normalize_params: normalize_params_for_mode(mode),
        })
    }

    /// Set classifier options
    pub fn with_options(mut self, options: OriOptions) -> Self {
        self.options = options;
        self.normalize_params = normalize_params_for_mode(self.options.preprocess_mode);
        self
    }

    /// Get current options
    pub fn options(&self) -> &OriOptions {
        &self.options
    }

    /// Modify options
    pub fn options_mut(&mut self) -> &mut OriOptions {
        &mut self.options
    }

    /// Classify a single text line image
    pub fn classify(&self, image: &DynamicImage) -> OcrResult<OrientationResult> {
        let input = preprocess_for_ori(
            image,
            self.options.target_height,
            self.options.target_width,
            self.options.resize_shorter,
            self.options.preprocess_mode,
            &self.normalize_params,
        )?;

        let output = self.engine.run_dynamic(input.view().into_dyn())?;
        self.decode_output(&output)
    }

    fn decode_output(&self, output: &ArrayD<f32>) -> OcrResult<OrientationResult> {
        let shape = output.shape();
        if shape.is_empty() {
            return Err(OcrError::PostprocessError(
                "Orientation model output shape is empty".to_string(),
            ));
        }

        let num_classes = *shape.last().unwrap_or(&0);
        if num_classes == 0 {
            return Err(OcrError::PostprocessError(
                "Orientation model output classes is zero".to_string(),
            ));
        }

        let output_data: Vec<f32> = output.iter().cloned().collect();
        if output_data.is_empty() {
            return Err(OcrError::PostprocessError(
                "Orientation model output data is empty".to_string(),
            ));
        }

        let scores_raw = if output_data.len() >= num_classes {
            output_data[..num_classes].to_vec()
        } else {
            return Err(OcrError::PostprocessError(
                "Orientation model output data size mismatch".to_string(),
            ));
        };

        let scores = softmax(&scores_raw);
        let (class_idx, &confidence) = scores
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .ok_or_else(|| {
                OcrError::PostprocessError(
                    "Orientation model output has no valid scores".to_string(),
                )
            })?;

        let angle = class_to_angle(num_classes, class_idx, &self.options.class_angles);
        Ok(OrientationResult::new(class_idx, angle, confidence, scores))
    }
}

/// Convert class index to angle in degrees (best effort mapping)
fn class_to_angle(num_classes: usize, class_idx: usize, class_angles: &[i32]) -> i32 {
    if class_angles.len() == num_classes {
        return class_angles
            .get(class_idx)
            .copied()
            .unwrap_or(class_idx as i32);
    }

    match num_classes {
        2 => {
            if class_idx == 0 {
                0
            } else {
                180
            }
        }
        4 => match class_idx {
            0 => 0,
            1 => 90,
            2 => 180,
            3 => 270,
            _ => class_idx as i32,
        },
        _ => class_idx as i32,
    }
}

fn softmax(scores: &[f32]) -> Vec<f32> {
    if scores.is_empty() {
        return Vec::new();
    }

    let max_score = scores.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let exp_scores: Vec<f32> = scores.iter().map(|&s| (s - max_score).exp()).collect();
    let sum_exp: f32 = exp_scores.iter().sum();

    if sum_exp == 0.0 {
        return vec![0.0; scores.len()];
    }

    exp_scores.into_iter().map(|v| v / sum_exp).collect()
}

fn normalize_params_for_mode(mode: OriPreprocessMode) -> NormalizeParams {
    match mode {
        OriPreprocessMode::Doc => NormalizeParams::paddle_det(),
        OriPreprocessMode::Textline => NormalizeParams::paddle_rec(),
    }
}

/// Preprocess image for orientation classification
fn preprocess_for_ori(
    img: &DynamicImage,
    target_height: u32,
    target_width: u32,
    resize_shorter: u32,
    mode: OriPreprocessMode,
    params: &NormalizeParams,
) -> OcrResult<Array4<f32>> {
    if target_height == 0 || target_width == 0 {
        return Err(OcrError::PreprocessError(
            "Target size must be greater than zero".to_string(),
        ));
    }

    let processed = match mode {
        OriPreprocessMode::Textline => {
            let (w, h) = img.dimensions();
            let ratio = w as f32 / h.max(1) as f32;
            let mut resize_w = (target_height as f32 * ratio).round() as u32;
            if resize_w == 0 {
                resize_w = 1;
            }
            if resize_w > target_width {
                resize_w = target_width;
            }

            img.resize_exact(
                resize_w,
                target_height,
                image::imageops::FilterType::Lanczos3,
            )
        }
        OriPreprocessMode::Doc => {
            let (w, h) = img.dimensions();
            let shorter = w.min(h).max(1) as f32;
            let scale = resize_shorter as f32 / shorter;
            let new_w = (w as f32 * scale).round().max(1.0) as u32;
            let new_h = (h as f32 * scale).round().max(1.0) as u32;
            let resized = img.resize_exact(new_w, new_h, image::imageops::FilterType::Lanczos3);

            if new_w < target_width || new_h < target_height {
                resized.resize_exact(
                    target_width,
                    target_height,
                    image::imageops::FilterType::Lanczos3,
                )
            } else {
                let left = (new_w - target_width) / 2;
                let top = (new_h - target_height) / 2;
                resized.crop_imm(left, top, target_width, target_height)
            }
        }
    };

    let rgb_img = processed.to_rgb8();
    let (proc_w, proc_h) = processed.dimensions();

    let mut input = Array4::<f32>::zeros((1, 3, target_height as usize, target_width as usize));

    let max_y = proc_h.min(target_height) as usize;
    let max_x = proc_w.min(target_width) as usize;

    for y in 0..max_y {
        for x in 0..max_x {
            let pixel = rgb_img.get_pixel(x as u32, y as u32);
            let [r, g, b] = pixel.0;

            // Paddle models use BGR channel order in most preprocessing pipelines.
            input[[0, 0, y, x]] = (b as f32 / 255.0 - params.mean[0]) / params.std[0];
            input[[0, 1, y, x]] = (g as f32 / 255.0 - params.mean[1]) / params.std[1];
            input[[0, 2, y, x]] = (r as f32 / 255.0 - params.mean[2]) / params.std[2];
        }
    }

    Ok(input)
}

/// Low-level orientation API
impl OriModel {
    /// Raw inference interface
    pub fn run_raw(&self, input: ndarray::ArrayViewD<f32>) -> OcrResult<ArrayD<f32>> {
        Ok(self.engine.run_dynamic(input)?)
    }

    /// Get model input shape
    pub fn input_shape(&self) -> &[usize] {
        self.engine.input_shape()
    }

    /// Get model output shape
    pub fn output_shape(&self) -> &[usize] {
        self.engine.output_shape()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ori_options_default() {
        let opts = OriOptions::default();
        assert_eq!(opts.target_height, 224);
        assert_eq!(opts.target_width, 224);
        assert_eq!(opts.min_score, 0.5);
        assert_eq!(opts.resize_shorter, 256);
        assert_eq!(opts.preprocess_mode, OriPreprocessMode::Doc);
        assert_eq!(opts.class_angles, vec![0, 90, 180, 270]);
    }

    #[test]
    fn test_ori_options_builder() {
        let opts = OriOptions::new()
            .with_target_height(32)
            .with_target_width(128)
            .with_min_score(0.7)
            .with_resize_shorter(200)
            .with_preprocess_mode(OriPreprocessMode::Textline)
            .with_class_angles(vec![0, 180]);

        assert_eq!(opts.target_height, 32);
        assert_eq!(opts.target_width, 128);
        assert_eq!(opts.min_score, 0.7);
        assert_eq!(opts.resize_shorter, 200);
        assert_eq!(opts.preprocess_mode, OriPreprocessMode::Textline);
        assert_eq!(opts.class_angles, vec![0, 180]);
    }

    #[test]
    fn test_class_to_angle_mapping() {
        let angles_4 = vec![0, 90, 180, 270];
        let angles_2 = vec![0, 180];
        assert_eq!(class_to_angle(2, 0, &angles_2), 0);
        assert_eq!(class_to_angle(2, 1, &angles_2), 180);
        assert_eq!(class_to_angle(4, 0, &angles_4), 0);
        assert_eq!(class_to_angle(4, 1, &angles_4), 90);
        assert_eq!(class_to_angle(4, 2, &angles_4), 180);
        assert_eq!(class_to_angle(4, 3, &angles_4), 270);
        assert_eq!(class_to_angle(3, 2, &angles_2), 2);
    }

    #[test]
    fn test_preprocess_for_ori_shape() {
        let img = DynamicImage::new_rgb8(100, 32);
        let params = NormalizeParams::paddle_det();
        let tensor =
            preprocess_for_ori(&img, 224, 224, 256, OriPreprocessMode::Doc, &params).unwrap();
        assert_eq!(tensor.shape(), &[1, 3, 224, 224]);
    }
}
