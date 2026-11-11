//! 本地大模型推理进程。主程序经标准输入逐行发 JSON 命令，这里逐行回 JSON 结果。
//! 独立成进程：推理库与语音识别运行时各带一份 ggml，不能进同一个可执行文件；
//! 显卡驱动出错也只影响这个进程，退出即释放全部显存。

#[cfg(windows)]
mod draft;
#[cfg(windows)]
mod engine;

#[cfg(windows)]
fn main() {
    use serde_json::{json, Value};
    use std::io::{BufRead, Write};
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{mpsc, Arc, Mutex};

    // 命令按到达顺序编号。让一次整理提前结束有两种情况：主程序点名取消它；
    // 或者它是边说边做的预先整理，而后面来了正式请求。正式请求之间互不打断，按顺序执行。
    let latest_final = Arc::new(AtomicU64::new(0));
    let withdrawn = Arc::new(Mutex::new(HashSet::<u64>::new()));
    let (sender, receiver) = mpsc::channel::<(u64, Value)>();
    let (reader_final, reader_withdrawn) = (latest_final.clone(), withdrawn.clone());
    std::thread::spawn(move || {
        let mut order = 0;
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            let Ok(command) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            order += 1;
            match command["op"].as_str() {
                Some("cancel") => {
                    if let (Some(target), Ok(mut withdrawn)) =
                        (command["target"].as_u64(), reader_withdrawn.lock())
                    {
                        withdrawn.insert(target);
                    }
                    continue;
                }
                Some("run") if command["speculative"].as_bool() != Some(true) => {
                    reader_final.store(order, Ordering::SeqCst);
                }
                _ => {}
            }
            if sender.send((order, command)).is_err() {
                break;
            }
        }
    });

    let mut engine: Option<engine::Engine> = None;
    for (order, command) in receiver {
        let id = command["id"].clone();
        let result: Result<Value, String> = match command["op"].as_str() {
            Some("load") => serde_json::from_value::<engine::LoadRequest>(command)
                .map_err(|error| error.to_string())
                .and_then(|request| {
                    let started = std::time::Instant::now();
                    engine = None;
                    let loaded = engine.insert(engine::Engine::load(&request)?);
                    Ok(json!({
                        "loadMs": started.elapsed().as_millis() as u64,
                        "gpu": loaded.on_gpu(),
                    }))
                }),
            Some("run") => serde_json::from_value::<engine::RunRequest>(command)
                .map_err(|error| error.to_string())
                .and_then(|request| {
                    let engine = engine.as_mut().ok_or("模型尚未加载")?;
                    let outcome = engine.run(&request, &|| {
                        (request.speculative && latest_final.load(Ordering::SeqCst) > order)
                            || withdrawn
                                .lock()
                                .is_ok_and(|withdrawn| id.as_u64().is_some_and(|id| withdrawn.contains(&id)))
                    })?;
                    serde_json::to_value(outcome).map_err(|error| error.to_string())
                }),
            _ => Err("未知命令".into()),
        };
        let mut reply = match result {
            Ok(value) => value,
            Err(error) => json!({"error": error}),
        };
        reply["id"] = id;
        let mut stdout = std::io::stdout().lock();
        if writeln!(stdout, "{reply}").and_then(|_| stdout.flush()).is_err() {
            break;
        }
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("本地大模型推理目前只支持 Windows");
    std::process::exit(1);
}
