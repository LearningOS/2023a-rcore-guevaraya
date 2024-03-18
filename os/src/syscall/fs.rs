//! File and filesystem-related syscalls
use crate::fs::{file_link, file_unlink, open_file, OpenFlags, Stat};
use crate::mm::{translated_byte_buffer, translated_str, UserBuffer};
use crate::task::{current_task, current_user_token};
use crate::fs::File;
use core::mem;
use core::slice::from_raw_parts;
pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_write", current_task().unwrap().pid.0);
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        if !file.writable() {
            return -1;
        }
        let file = file.clone();
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        file.write(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else {
        -1
    }
}

pub fn sys_read(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_read", current_task().unwrap().pid.0);
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        let file: alloc::sync::Arc<dyn File + Send + Sync> = file.clone();
        if !file.readable() {
            return -1;
        }
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        trace!("kernel: sys_read .. file.read");
        file.read(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else {
        -1
    }
}

pub fn sys_open(path: *const u8, flags: u32) -> isize {
    info!("kernel:pid[{}] sys_open", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(inode) = open_file(path.as_str(), OpenFlags::from_bits(flags).unwrap()) {
        let mut inner = task.inner_exclusive_access();
        let fd = inner.alloc_fd();
        inner.fd_table[fd] = Some(inode);
        fd as isize
    } else {
        -1
    }
}

pub fn sys_close(fd: usize) -> isize {
    trace!("kernel:pid[{}] sys_close", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if inner.fd_table[fd].is_none() {
        return -1;
    }
    inner.fd_table[fd].take();
    0
}

/// YOUR JOB: Implement fstat.
pub fn sys_fstat(fd: usize, _st: *mut Stat) -> isize {
    info!(
        "kernel:pid[{}] sys_fstat IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        log::error!(" fd invaild {:?}", fd);
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        let file: alloc::sync::Arc<dyn File + Send + Sync> = file.clone();
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        trace!("kernel: sys_read .. file.read");
        let buffers = translated_byte_buffer(
            current_user_token(),
            _st as *mut u8,
            mem::size_of::<Stat>(),
        );
        let k_stat = file.stat().unwrap();
        let mut position: usize = 0;
        let slice_stat= unsafe {
            from_raw_parts(
                &k_stat as *const Stat as *const u8,
                mem::size_of::<Stat>(),
            )
        };

        for ker_addr_set in buffers {
            let pslice_ts = &slice_stat[position..ker_addr_set.len()];
            position += ker_addr_set.len();
            ker_addr_set.copy_from_slice(pslice_ts);
            //trace!("ker_addr_set.len:{}", ker_addr_set.len());
        }
        0
    } else {
        log::error!(" file not open/notfound");
        -1
    }
}

/// YOUR JOB: Implement linkat.
pub fn sys_linkat(_old_name: *const u8, _new_name: *const u8) -> isize {
    trace!(
        "kernel:pid[{}] sys_linkat IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    info!("kernel:pid[{}] sys_link", current_task().unwrap().pid.0);
    //let task = current_task().unwrap();
    let token = current_user_token();
    let old_path = translated_str(token, _old_name);
    let new_path = translated_str(token, _new_name);
    return file_link(&old_path, &new_path);
}

/// YOUR JOB: Implement unlinkat.
pub fn sys_unlinkat(_name: *const u8) -> isize {
    trace!(
        "kernel:pid[{}] sys_unlinkat IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    info!("kernel:pid[{}] sys_unlink", current_task().unwrap().pid.0);
    //let task = current_task().unwrap();
    let token = current_user_token();
    let path = translated_str(token, _name);
    if file_unlink(&path).is_some() {
        info!("kernel: unlink ok");    
        0
    } else {
        info!("kernel: unlink failed");       
        -1
    }
}
