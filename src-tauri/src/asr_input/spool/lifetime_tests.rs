use super::*;
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

// 仅清理本测试创建的独立目录；失败路径也必须终止并回收明确的子进程。
struct TestProcess {
    child: Child,
    root: PathBuf,
}
impl Drop for TestProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        for entry in std::fs::read_dir(&self.root).unwrap() {
            let path = entry.unwrap().path();
            // 目录里只允许本测试的就绪标记及本测试创建的暂存文件。
            let name = path.file_name().unwrap().to_string_lossy();
            assert!(
                name == "ready"
                    || name.starts_with("say-it-asr-queue-")
                    || name.starts_with("say-it-audio-lab-")
            );
            std::fs::remove_file(path).unwrap();
        }
        std::fs::remove_dir(&self.root).unwrap();
    }
}

#[test]
fn operating_system_reclaims_files_after_forced_exit_without_rust_drop() {
    for _ in 0..3 {
        // Unix 可读试听路径只保证正常析构清理，不能套用 Windows 的关闭删除契约。
        let cases: &[&str] = if cfg!(windows) {
            &["empty", "written", "read", "exit", "preview"]
        } else {
            &["empty", "written", "read", "exit"]
        };
        for &case in cases {
            let root = std::env::temp_dir()
                .join(format!("say-it-spool-lifetime-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&root).unwrap();
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "asr_input::spool::lifetime_tests::lifetime_child",
                    "--ignored",
                    "--test-threads=1",
                ])
                .env("SAYIT_SPOOL_LIFETIME_ROOT", &root)
                .env("SAYIT_SPOOL_LIFETIME_CASE", case)
                .env("TMP", &root)
                .env("TEMP", &root)
                .env("TMPDIR", &root)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                use windows::Win32::System::Threading::CREATE_NO_WINDOW;
                command.creation_flags(CREATE_NO_WINDOW.0);
            }
            let child = command.spawn().unwrap();
            let mut process = TestProcess { child, root };
            let deadline = Instant::now() + Duration::from_secs(10);
            while !process.root.join("ready").exists() {
                assert!(Instant::now() < deadline, "子进程未就绪");
                if let Some(status) = process.child.try_wait().unwrap() {
                    assert!(
                        process.root.join("ready").exists(),
                        "子进程提前结束：{status}"
                    );
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            #[cfg(windows)]
            if case != "exit" {
                assert!(
                    std::fs::read_dir(&process.root).unwrap().count() > 1,
                    "必须在文件仍被持有时强制结束"
                );
            }
            if case != "exit" {
                process.child.kill().unwrap();
            }
            let status = process.child.wait().unwrap();
            assert!(!status.success());
            if case == "exit" {
                assert_eq!(status.code(), Some(73));
            }
            // 在清理守卫执行前验证文件已由 OS 删除，不能用测试自己的清理掩盖失败。
            let files: Vec<_> = std::fs::read_dir(&process.root)
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .filter(|name| name != "ready")
                .collect();
            assert!(files.is_empty(), "进程退出仍有暂存文件：{files:?}");
        }
    }
}

#[test]
#[ignore = "仅由生命周期回归启动的隔离子进程；不在主测试进程执行强制退出"]
fn lifetime_child() {
    let Ok(root) = std::env::var("SAYIT_SPOOL_LIFETIME_ROOT") else {
        return;
    };
    let case = std::env::var("SAYIT_SPOOL_LIFETIME_CASE").unwrap();
    assert_eq!(std::env::temp_dir(), PathBuf::from(&root));
    let file = if case == "empty" {
        Some(TemporaryFile::create().unwrap())
    } else if case == "preview" {
        let file = TemporaryFile::create_readable().unwrap();
        file.write_all_at(&[0x25; 8192], 0).unwrap();
        assert_eq!(
            std::fs::read(file.readable_path()).unwrap(),
            vec![0x25; 8192]
        );
        Some(file)
    } else {
        None
    };
    let mut writer = Writer::default();
    let mut packets = Vec::new();
    if case != "empty" && case != "preview" {
        for sequence in 0..520 {
            packets.push(
                writer
                    .write(&vec![sequence as f32; 4096], || false)
                    .unwrap(),
            );
        }
        assert_eq!(writer.paths.lock().unwrap().len(), 2);
        if case == "read" {
            let output = Reader.read(packets.remove(0)).unwrap();
            assert!(output.iter().all(|sample| *sample == 0.0));
        }
    }
    std::fs::write(PathBuf::from(root).join("ready"), b"ready").unwrap();
    if case == "exit" {
        std::process::exit(73);
    }
    // 父进程持有管道并直接 kill；这里不会先返回并执行析构。
    std::io::stdin().read_exact(&mut [0u8]).unwrap();
    std::hint::black_box((file, writer, packets));
    panic!("父进程应直接终止测试子进程");
}

#[test]
fn explicit_offsets_preserve_audio_during_concurrent_reads_and_writes() {
    let (tx, rx) = std::sync::mpsc::sync_channel(8);
    let producer = std::thread::spawn(move || {
        let mut writer = Writer::default();
        for sequence in 0..1000 {
            let mut samples = vec![sequence as f32; 4096];
            samples[0] = f32::from_bits(0x7fc01234);
            samples[1] = -0.0;
            tx.send(writer.write(&samples, || false).unwrap()).unwrap();
        }
        let paths = writer.paths.lock().unwrap().clone();
        paths
    });
    let mut reader = Reader;
    for (sequence, packet) in rx.into_iter().enumerate() {
        let output = reader.read(packet).unwrap();
        assert_eq!(output[0].to_bits(), 0x7fc01234);
        assert_eq!(output[1].to_bits(), (-0.0f32).to_bits());
        assert!(output[2..].iter().all(|sample| *sample == sequence as f32));
    }
    let paths = producer.join().unwrap();
    assert!(paths.iter().all(|path| !path.exists()));
}

#[test]
fn temporary_audio_cannot_be_reopened_by_path() {
    let file = TemporaryFile::create().unwrap();
    file.write_all_at(&[1, 2, 3, 4], 0).unwrap();
    assert!(std::fs::File::open(file.path()).is_err());
    assert!(std::fs::OpenOptions::new()
        .write(true)
        .open(file.path())
        .is_err());
    let mut output = [0; 4];
    file.read_exact_at(&mut output, 0).unwrap();
    assert_eq!(output, [1, 2, 3, 4]);
}
