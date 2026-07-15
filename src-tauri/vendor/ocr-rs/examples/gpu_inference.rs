//! GPU 推理示例
//!
//! GPU Inference Example
//!
//! 展示如何配置和使用 GPU 加速

use ocr_rs::{Backend, DetOptions, OcrEngine, OcrEngineConfig};
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
            "用法: {} <图像路径> <检测模型> <识别模型> <字符集> [后端]",
            args[0]
        );
        eprintln!("后端选项: cpu, metal, opencl, vulkan");
        std::process::exit(1);
    }

    let image_path = &args[1];
    let det_model_path = &args[2];
    let rec_model_path = &args[3];
    let charset_path = &args[4];
    let backend_str = args.get(5).map(|s| s.as_str()).unwrap_or("cpu");

    // 选择后端
    let backend = match backend_str.to_lowercase().as_str() {
        "cpu" => Backend::CPU,
        "metal" => Backend::Metal,
        "opencl" => Backend::OpenCL,
        "vulkan" => Backend::Vulkan,
        "opengl" => Backend::OpenGL,
        "cuda" => Backend::CUDA,
        _ => {
            eprintln!("未知后端: {}, 使用 CPU", backend_str);
            Backend::CPU
        }
    };

    println!("=== GPU 推理示例 ===\n");
    println!("使用后端: {:?}", backend);

    // 配置引擎
    let config = OcrEngineConfig::new()
        .with_backend(backend)
        .with_threads(4)
        .with_det_options(DetOptions::fast());

    println!("创建 OCR 引擎...");
    let create_start = Instant::now();

    let engine = OcrEngine::new(det_model_path, rec_model_path, charset_path, Some(config))?;

    let create_time = create_start.elapsed();
    println!("引擎创建耗时: {:?}\n", create_time);

    // 加载图像
    let image = image::open(image_path)?;
    println!(
        "图像: {} ({}x{})\n",
        image_path,
        image.width(),
        image.height()
    );

    // 预热 (首次推理通常较慢)
    println!("预热推理...");
    let _ = engine.recognize(&image)?;

    // 正式推理
    println!("执行 OCR...");
    let infer_start = Instant::now();
    let results = engine.recognize(&image)?;
    let infer_time = infer_start.elapsed();

    // 输出结果
    println!("\n识别结果 ({} 个):", results.len());
    println!("{:-<50}", "");

    for result in &results {
        println!(
            "- {} (置信度: {:.1}%)",
            result.text,
            result.confidence * 100.0
        );
    }

    println!("\n{:-<50}", "");
    println!("推理耗时: {:?}", infer_time);
    println!(
        "吞吐量: {:.1} 区域/秒",
        results.len() as f64 / infer_time.as_secs_f64()
    );

    // 多次推理性能测试
    println!("\n性能测试 (10次推理)...");
    let bench_start = Instant::now();
    for _ in 0..10 {
        let _ = engine.recognize(&image)?;
    }
    let bench_time = bench_start.elapsed();

    println!("总耗时: {:?}", bench_time);
    println!("平均耗时: {:?}", bench_time / 10);

    Ok(())
}
