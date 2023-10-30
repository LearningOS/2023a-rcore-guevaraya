//! Process management syscalls
use crate::{
    task::{
        change_program_brk, exit_current_and_run_next, suspend_current_and_run_next,
        task_syscall_mmap,task_syscall_munmap
    },
    task::{sys_get_task_status, TaskInfo},
};
use crate::timer::{get_time_ms,get_time_us};
use crate::mm::translated_byte_buffer;
use crate::task::current_user_token;
use core::slice::from_raw_parts;
use core::mem;

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}


/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let buffers = translated_byte_buffer(current_user_token(), _ts as *mut u8, mem::size_of::<TimeVal>());
    let k_ts = TimeVal{sec:get_time_ms()/1000, usec:get_time_us()};
    let mut position:usize = 0;
    let slice_ts = unsafe { from_raw_parts(&k_ts as *const TimeVal as *const u8, mem::size_of::<TimeVal>())};
    //debug!("get_time_ms:{:?}",get_time_ms());
    for ker_addr_set in buffers{
        let pslice_ts = &slice_ts[position..ker_addr_set.len()];
        position += ker_addr_set.len() ;
        ker_addr_set.copy_from_slice(pslice_ts);
        trace!("ker_addr_set.len:{}", ker_addr_set.len());
    }
    let ret: isize = position as isize;
    //debug!("ret:{}tz:{} position: {}", ret,_tz, position);
    if ret <= 0 {
        debug!("_tz:{}", _tz);
        panic!("kernel: position:{}", position);
    }
    0

}

/// YOUR JOB: Finish sys_task_info to pass testcases
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TaskInfo`] is splitted by two pages ?
pub fn sys_task_info(_ti: *mut TaskInfo) -> isize {
    trace!("kernel: sys_task_info");

    let buffers = translated_byte_buffer(current_user_token(), _ti as *mut u8, mem::size_of::<TaskInfo>());
    let mut position:usize = 0;
    let mut info: TaskInfo = sys_get_task_status();
    let slice_info = unsafe { from_raw_parts(&info as *const TaskInfo as *const u8, mem::size_of::<TaskInfo>())};
    info.time = get_time_us()/1000 - info.time;
    debug!("sys_task_info:{:?}",get_time_us()/1000);   
    for ker_addr_set in buffers{
        let pslice_ts = &slice_info[position..ker_addr_set.len()];
        position += ker_addr_set.len() ;
        ker_addr_set.copy_from_slice(pslice_ts);
        trace!("ker_addr_set.len:{}", ker_addr_set.len());
    }
    0
}

// YOUR JOB: Implement mmap.
pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
    debug!("kernel: sys_mmap {:#x} len:{:#x} port:{:#x}",_start, _len, _port);
    let ret = task_syscall_mmap(_start, _len, _port);
    debug!("sys_mmap ret:{:?}", ret);
    ret
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    debug!("kernel: sys_munmap start:{:#x} len:{:#x}",_start, _len);
    let ret =task_syscall_munmap(_start, _len);
    debug!("sys_munmap ret:{:?}", ret);
    ret
}
/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}
