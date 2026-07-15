//! 调试检测模型输出

use image::GenericImageView;
use ocr_rs::mnn::{InferenceConfig, InferenceEngine};
use ocr_rs::postprocess::extract_boxes_from_mask_with_padding;
use ocr_rs::preprocess::{preprocess_for_det, NormalizeParams};
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("用法: debug_det <模型路径> <图像路径>");
        return;
    }

    let model_path = &args[1];
    let image_path = &args[2];

    println!("加载模型: {}", model_path);
    let config = InferenceConfig::default();
    let engine = InferenceEngine::from_file(model_path, Some(config)).unwrap();

    println!("模型输入形状: {:?}", engine.input_shape());
    println!("模型输出形状: {:?}", engine.output_shape());
    println!("是否动态形状: {}", engine.has_dynamic_shape());

    println!("\n加载图像: {}", image_path);
    let image = image::open(image_path).unwrap();
    let (original_w, original_h) = image.dimensions();
    println!("图像尺寸: {}x{}", original_w, original_h);

    // 缩放图像
    let max_side_len = 960u32;
    let max_dim = original_w.max(original_h);
    let (scaled, scaled_w, scaled_h) = if max_dim > max_side_len {
        let scale = max_side_len as f64 / max_dim as f64;
        let new_w = (original_w as f64 * scale).round() as u32;
        let new_h = (original_h as f64 * scale).round() as u32;
        println!("缩放到: {}x{}", new_w, new_h);
        (
            image.resize_exact(new_w, new_h, image::imageops::FilterType::Lanczos3),
            new_w,
            new_h,
        )
    } else {
        (image.clone(), original_w, original_h)
    };

    // 预处理
    let params = NormalizeParams::paddle_det();
    let input = preprocess_for_det(&scaled, &params).expect("预处理失败");
    println!("输入张量形状: {:?}", input.shape());

    // 推理
    println!("\n执行推理...");
    let output = engine.run_dynamic(input.view().into_dyn()).unwrap();
    let output_shape = output.shape();
    println!("输出张量形状: {:?}", output_shape);

    let out_w = output_shape[3] as u32;
    let out_h = output_shape[2] as u32;
    println!("输出尺寸: {}x{}", out_w, out_h);
    println!("有效尺寸: {}x{}", scaled_w, scaled_h);

    // 分析输出
    let output_data: Vec<f32> = output.iter().cloned().collect();
    let min_val = output_data.iter().cloned().fold(f32::INFINITY, f32::min);
    let max_val = output_data
        .iter()
        .cloned()
        .fold(f32::NEG_INFINITY, f32::max);
    let mean_val: f32 = output_data.iter().sum::<f32>() / output_data.len() as f32;

    println!("输出值范围: [{:.6}, {:.6}]", min_val, max_val);
    println!("输出平均值: {:.6}", mean_val);

    // 使用不同的阈值测试边界框提取
    let thresholds = [0.1, 0.2, 0.3, 0.4, 0.5];
    println!("\n边界框提取测试:");
    for thresh in thresholds {
        // 二值化
        let binary_mask: Vec<u8> = output_data
            .iter()
            .map(|&v| if v > thresh { 255u8 } else { 0u8 })
            .collect();

        let boxes = extract_boxes_from_mask_with_padding(
            &binary_mask,
            out_w,
            out_h,
            scaled_w,
            scaled_h,
            original_w,
            original_h,
            16,  // min_area
            0.5, // box_threshold
        );

        println!("  阈值 {:.1}: {} 个边界框", thresh, boxes.len());
        for (i, b) in boxes.iter().take(3).enumerate() {
            println!(
                "    [{i}] ({}, {}) {}x{}",
                b.rect.left(),
                b.rect.top(),
                b.rect.width(),
                b.rect.height()
            );
        }
    }
}
