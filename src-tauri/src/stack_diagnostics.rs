#[cfg(windows)]
mod windows_impl {
    use std::ffi::c_void;
    use std::fs::{File, OpenOptions};
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::{FromRawHandle, IntoRawHandle};
    use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU8, AtomicUsize, Ordering};

    use tauri::Manager;
    const STATUS_STACK_OVERFLOW: u32 = 0xc00000fd;
    const EXCEPTION_CONTINUE_SEARCH: i32 = 0;
    const FILE_FLAG_WRITE_THROUGH: u32 = 0x80000000;
    const STACK_GUARANTEE_BYTES: u32 = 64 * 1024;
    const THREAD_SLOT_COUNT: usize = 256;
    const THREAD_NAME_BYTES: usize = 64;
    const MAX_FRAMES: usize = 64;
    const RECORD_BYTES: usize = 4096;

    static INSTALLED: AtomicBool = AtomicBool::new(false);
    static HANDLING: AtomicBool = AtomicBool::new(false);
    static STACK_LOG_HANDLE: AtomicIsize = AtomicIsize::new(0);

    #[repr(C)]
    struct ExceptionPointers {
        exception_record: *mut ExceptionRecord,
        context_record: *mut c_void,
    }

    #[repr(C)]
    struct ExceptionRecord {
        exception_code: u32,
        exception_flags: u32,
        exception_record: *mut ExceptionRecord,
        exception_address: *mut c_void,
        number_parameters: u32,
        exception_information: [usize; 15],
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn AddVectoredExceptionHandler(
            first: u32,
            handler: Option<unsafe extern "system" fn(*mut ExceptionPointers) -> i32>,
        ) -> *mut c_void;
        fn GetCurrentThreadId() -> u32;
        fn GetModuleHandleW(module_name: *const u16) -> *mut c_void;
        fn SetThreadStackGuarantee(stack_size_in_bytes: *mut u32) -> i32;
        fn WriteFile(
            file: *mut c_void,
            buffer: *const c_void,
            bytes_to_write: u32,
            bytes_written: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
    }

    #[link(name = "ntdll")]
    extern "system" {
        fn RtlCaptureStackBackTrace(
            frames_to_skip: u32,
            frames_to_capture: u32,
            backtrace: *mut *mut c_void,
            backtrace_hash: *mut u32,
        ) -> u16;
    }

    struct ThreadSlot {
        id: AtomicU32,
        name_len: AtomicUsize,
        name: [AtomicU8; THREAD_NAME_BYTES],
    }

    impl ThreadSlot {
        const fn new() -> Self {
            Self {
                id: AtomicU32::new(0),
                name_len: AtomicUsize::new(0),
                name: [const { AtomicU8::new(0) }; THREAD_NAME_BYTES],
            }
        }
    }

    static THREADS: [ThreadSlot; THREAD_SLOT_COUNT] =
        [const { ThreadSlot::new() }; THREAD_SLOT_COUNT];

    struct EmergencyRecord {
        bytes: [u8; RECORD_BYTES],
        len: usize,
    }

    impl EmergencyRecord {
        fn new() -> Self {
            Self {
                bytes: [0; RECORD_BYTES],
                len: 0,
            }
        }

        fn push(&mut self, bytes: &[u8]) {
            let count = bytes.len().min(RECORD_BYTES.saturating_sub(self.len));
            self.bytes[self.len..self.len + count].copy_from_slice(&bytes[..count]);
            self.len += count;
        }

        fn push_decimal(&mut self, mut value: u32) {
            let mut digits = [0_u8; 10];
            let mut start = digits.len();
            loop {
                start -= 1;
                digits[start] = b'0' + (value % 10) as u8;
                value /= 10;
                if value == 0 {
                    break;
                }
            }
            self.push(&digits[start..]);
        }

        fn push_hex(&mut self, value: usize) {
            const HEX: &[u8; 16] = b"0123456789abcdef";
            self.push(b"0x");
            for shift in (0..usize::BITS).step_by(4).rev() {
                self.push(&[HEX[((value >> shift) & 0xf) as usize]]);
            }
        }
    }

    pub(crate) fn prepare_current_thread() {
        let mut guarantee = STACK_GUARANTEE_BYTES;
        unsafe {
            SetThreadStackGuarantee(&mut guarantee);
        }
        let id = unsafe { GetCurrentThreadId() };
        let slot = THREADS
            .iter()
            .find(|slot| slot.id.load(Ordering::Acquire) == id)
            .or_else(|| {
                THREADS.iter().find(|slot| {
                    slot.id
                        .compare_exchange(0, id, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok()
                })
            })
            .unwrap_or(&THREADS[id as usize % THREAD_SLOT_COUNT]);
        slot.name_len.store(0, Ordering::Release);
        slot.id.store(id, Ordering::Relaxed);
        let current_thread = std::thread::current();
        let name = current_thread.name().unwrap_or("unnamed");
        let bytes = name.as_bytes();
        let count = bytes.len().min(THREAD_NAME_BYTES);
        for (target, value) in slot.name.iter().zip(bytes.iter()).take(count) {
            target.store(*value, Ordering::Relaxed);
        }
        slot.name_len.store(count, Ordering::Release);
    }

    pub(crate) fn forget_current_thread() {
        let id = unsafe { GetCurrentThreadId() };
        if let Some(slot) = THREADS
            .iter()
            .find(|slot| slot.id.load(Ordering::Acquire) == id)
        {
            slot.name_len.store(0, Ordering::Release);
            slot.id.store(0, Ordering::Release);
        }
    }

    pub(crate) fn install(app: &tauri::AppHandle) -> Result<(), String> {
        if INSTALLED.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let directory = app
            .path()
            .app_log_dir()
            .map_err(|error| format!("定位栈诊断目录失败：{error}"))?;
        std::fs::create_dir_all(&directory)
            .map_err(|error| format!("创建栈诊断目录失败：{error}"))?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .custom_flags(FILE_FLAG_WRITE_THROUGH)
            .open(directory.join("say-it-stack-overflow.log"))
            .map_err(|error| format!("打开栈溢出诊断日志失败：{error}"))?;
        let raw = file.into_raw_handle() as isize;
        STACK_LOG_HANDLE.store(raw, Ordering::Release);
        let handler = unsafe { AddVectoredExceptionHandler(1, Some(handle_exception)) };
        if handler.is_null() {
            STACK_LOG_HANDLE.store(0, Ordering::Release);
            unsafe {
                drop(File::from_raw_handle(raw as *mut c_void));
            }
            INSTALLED.store(false, Ordering::Release);
            return Err("注册 Windows 栈溢出诊断处理器失败".into());
        }
        prepare_current_thread();
        Ok(())
    }

    unsafe extern "system" fn handle_exception(info: *mut ExceptionPointers) -> i32 {
        if info.is_null()
            || (*info).exception_record.is_null()
            || (*(*info).exception_record).exception_code != STATUS_STACK_OVERFLOW
            || HANDLING.swap(true, Ordering::AcqRel)
        {
            return EXCEPTION_CONTINUE_SEARCH;
        }

        let id = GetCurrentThreadId();
        let slot = THREADS
            .iter()
            .find(|slot| slot.id.load(Ordering::Acquire) == id);
        let mut record = EmergencyRecord::new();
        record.push(b"stack_overflow tid=");
        record.push_decimal(id);
        record.push(b" name=");
        if let Some(slot) = slot {
            let count = slot.name_len.load(Ordering::Acquire).min(THREAD_NAME_BYTES);
            for value in slot.name.iter().take(count) {
                record.push(&[value.load(Ordering::Relaxed)]);
            }
        } else {
            record.push(b"unregistered");
        }
        record.push(b" module_base=");
        record.push_hex(GetModuleHandleW(std::ptr::null()) as usize);
        record.push(b" exception=");
        record.push_hex((*(*info).exception_record).exception_address as usize);
        record.push(b" frames=");
        if slot.is_some() {
            let mut frames = [std::ptr::null_mut(); MAX_FRAMES];
            let count = RtlCaptureStackBackTrace(
                0,
                MAX_FRAMES as u32,
                frames.as_mut_ptr(),
                std::ptr::null_mut(),
            ) as usize;
            for (index, frame) in frames.iter().take(count).enumerate() {
                if index > 0 {
                    record.push(b",");
                }
                record.push_hex(*frame as usize);
            }
        }
        record.push(b"\n");

        let raw = STACK_LOG_HANDLE.load(Ordering::Acquire);
        if raw != 0 {
            let mut written = 0;
            WriteFile(
                raw as *mut c_void,
                record.bytes.as_ptr().cast(),
                record.len as u32,
                &mut written,
                std::ptr::null_mut(),
            );
        }
        EXCEPTION_CONTINUE_SEARCH
    }
}

#[cfg(windows)]
pub(crate) use windows_impl::{forget_current_thread, install, prepare_current_thread};

#[cfg(not(windows))]
pub(crate) fn prepare_current_thread() {}

#[cfg(not(windows))]
pub(crate) fn forget_current_thread() {}

#[cfg(not(windows))]
pub(crate) fn install(_app: &tauri::AppHandle) -> Result<(), String> {
    Ok(())
}
