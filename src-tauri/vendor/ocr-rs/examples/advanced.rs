//! 高级使用示例
//!
//! Advanced Usage Example
//!
//! 展示如何使用底层 API 进行更精细的控制

use ocr_rs::{
    DetModel, DetOptions, DetPrecisionMode, InferenceConfig, PrecisionMode, RecModel, RecOptions,
};
use std::env;
use std::error::Error;
use std::time::Instant;

fn main() -> Result<(), Box<dyn Error>> {
    // 初始化日志
    env_logger::init();

    // 获取命令行参数
    let args: Vec<String> = env::args().collect();
    if args.len() < 5 {
        eprintln!(
            "用法: {} <图像路径> <检测模型> <识别模型> <字符集>",
            args[0]
        );
        std::process::exit(1);
    }

    let image_path = &args[1];
    let det_model_path = &args[2];
    let rec_model_path = &args[3];
    let charset_path = &args[4];

    // 配置推理引擎
    let inference_config = InferenceConfig::new()
        .with_threads(4)
        .with_precision(PrecisionMode::Normal);

    println!("=== 高级 OCR 示例 ===\n");

    // 1. 创建检测模型 (使用高精度模式)
    println!("加载检测模型...");
    let det_options = DetOptions::new()
        .with_max_side_len(1280)
        .with_precision_mode(DetPrecisionMode::Fast)
        .with_box_threshold(0.5)
        .with_score_threshold(0.3)
        .with_merge_boxes(true)
        .with_merge_threshold(10);

    let det_model = DetModel::from_file(det_model_path, Some(inference_config.clone()))?
        .with_options(det_options);

    // 2. 创建识别模型
    println!("加载识别模型...");
    let rec_options = RecOptions::new().with_min_score(0.5).with_batch_size(8);

    let rec_model = RecModel::from_file(rec_model_path, charset_path, Some(inference_config))?
        .with_options(rec_options);

    println!("字符集大小: {}\n", rec_model.charset_size());

    // 3. 加载图像
    println!("加载图像: {}", image_path);
    let image = image::open(image_path)?;
    println!("图像大小: {}x{}\n", image.width(), image.height());

    // 4. 执行检测
    println!("执行文本检测...");
    let det_start = Instant::now();
    let detections = det_model.detect_and_crop(&image)?;
    let det_time = det_start.elapsed();

    println!("检测耗时: {:?}", det_time);
    println!("检测到 {} 个文本区域\n", detections.len());

    if detections.is_empty() {
        println!("未检测到文本区域");
        return Ok(());
    }

    // 5. 执行识别
    println!("执行文本识别...");
    let rec_start = Instant::now();

    let images: Vec<_> = detections.iter().map(|(img, _)| img.clone()).collect();
    let boxes: Vec<_> = detections.iter().map(|(_, bbox)| bbox.clone()).collect();

    let rec_results = rec_model.recognize_batch(&images)?;
    let rec_time = rec_start.elapsed();

    println!("识别耗时: {:?}\n", rec_time);

    // 6. 输出详细结果
    println!("{:=<60}", "");
    println!("识别结果:");
    println!("{:=<60}\n", "");

    for (i, (result, bbox)) in rec_results.iter().zip(boxes.iter()).enumerate() {
        if result.text.is_empty() {
            continue;
        }

        println!("[区域 {}]", i + 1);
        println!("  位置: ({}, {})", bbox.rect.left(), bbox.rect.top());
        println!("  大小: {}x{}", bbox.rect.width(), bbox.rect.height());
        println!("  文本: {}", result.text);
        println!("  置信度: {:.1}%", result.confidence * 100.0);

        // 显示字符级置信度
        if !result.char_scores.is_empty() {
            print!("  字符分数: ");
            for (ch, score) in &result.char_scores {
                print!("{}({:.0}%) ", ch, score * 100.0);
            }
            println!();
        }
        println!();
    }

    // 7. 统计信息
    println!("{:=<60}", "");
    println!("统计信息:");
    println!("  检测耗时: {:?}", det_time);
    println!("  识别耗时: {:?}", rec_time);
    println!("  总耗时: {:?}", det_time + rec_time);
    println!(
        "  平均每区域: {:?}",
        (det_time + rec_time) / (rec_results.len() as u32).max(1)
    );

    Ok(())
}
