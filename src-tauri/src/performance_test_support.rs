//! Windows 性能用例共用的进程计数器；每次采样需在独立进程中运行。
use std::ffi::c_void;

#[repr(C)]
#[derive(Default)]
pub(crate) struct MemoryCounters {
    size: u32,
    page_fault_count: u32,
    pub peak_working_set: usize,
    working_set: usize,
    peak_paged_pool: usize,
    paged_pool: usize,
    peak_nonpaged_pool: usize,
    nonpaged_pool: usize,
    pagefile_usage: usize,
    pub peak_pagefile_usage: usize,
    pub private_usage: usize,
}

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcess() -> *mut c_void;
    fn GetCurrentThread() -> *mut c_void;
    fn QueryThreadCycleTime(thread: *mut c_void, cycles: *mut u64) -> i32;
    fn K32GetProcessMemoryInfo(
        process: *mut c_void,
        counters: *mut MemoryCounters,
        size: u32,
    ) -> i32;
}

pub(crate) fn thread_cycles() -> u64 {
    let mut cycles = 0;
    assert_ne!(
        unsafe { QueryThreadCycleTime(GetCurrentThread(), &mut cycles) },
        0
    );
    cycles
}

pub(crate) fn memory() -> MemoryCounters {
    let mut counters = MemoryCounters {
        size: std::mem::size_of::<MemoryCounters>() as u32,
        ..Default::default()
    };
    // 使用当前进程伪句柄；结构与 Windows PROCESS_MEMORY_COUNTERS_EX 一致。
    let size = counters.size;
    assert_ne!(
        unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, size) },
        0
    );
    counters
}
