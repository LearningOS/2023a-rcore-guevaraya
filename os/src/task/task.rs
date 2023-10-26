//! Types related to task management & Functions for completely changing TCB
use super::TaskContext;
use super::{kstack_alloc, pid_alloc, KernelStack, PidHandle};
use crate::config::TRAP_CONTEXT_BASE;
use crate::mm::{MemorySet, PhysPageNum, VirtAddr, KERNEL_SPACE};
use crate::sync::UPSafeCell;
use crate::trap::{trap_handler, TrapContext};
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::cell::RefMut;
//use core::cmp::Ordering;
pub use crate::config::MAX_STRIDE_NUM;
/* 
struct Stride(u64);

impl PartialOrd for Stride {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if self.0 == other.0{
            Some(Ordering::Equal)
        } else if self.0 < other.0 && (other.0 - self.0) < MAX_STRIDE_NUM as u64/2 {
            Some(Ordering::Less)
        }else{
            Some(Ordering::Greater)
        }
    }
}
impl PartialEq for Stride {
    fn eq(&self, other: &Self) -> bool {
        false
    }
}
*/

/// Task control block structure
///
/// Directly save the contents that will not change during running

use crate::mm::{
    MapPermission,VPNRange,
};
use crate::timer::get_time_ms;
pub use crate::config::MAX_SYSCALL_NUM;
/// Task information
#[allow(dead_code)]
#[derive(Copy, Clone)]
pub struct TaskInfo {
    /// Task status in it's life cycle
    pub status: TaskStatus,
    /// The numbers of syscall called by task
    pub syscall_times: [u32; MAX_SYSCALL_NUM],
    /// Total running time of task
    pub time: usize,
}
/// The task control block (TCB) of a task.
pub struct TaskControlBlock {
    // Immutable
    /// Process identifier
    pub pid: PidHandle,

    /// Kernel stack corresponding to PID
    pub kernel_stack: KernelStack,

    /// Mutable
    inner: UPSafeCell<TaskControlBlockInner>,
}

impl TaskControlBlock {
    /// Get the mutable reference of the inner TCB
    pub fn inner_exclusive_access(&self) -> RefMut<'_, TaskControlBlockInner> {
        self.inner.exclusive_access()
    }
    /// Get the address of app's page table
    pub fn get_user_token(&self) -> usize {
        let inner = self.inner_exclusive_access();
        inner.memory_set.token()
    }
}

pub struct TaskControlBlockInner {
    /// The physical page number of the frame where the trap context is placed
    pub trap_cx_ppn: PhysPageNum,

    /// Application data can only appear in areas
    /// where the application address space is lower than base_size
    pub base_size: usize,

    /// Save task context
    pub task_cx: TaskContext,

    /// Maintain the execution status of the current process
    pub task_status: TaskStatus,

    /// The task information
    pub task_info: TaskInfo,
    /// The first start timestamp
    pub timestamp: usize,

    /// Application address space
    pub memory_set: MemorySet,

    /// Parent process of the current process.
    /// Weak will not affect the reference count of the parent
    pub parent: Option<Weak<TaskControlBlock>>,

    /// A vector containing TCBs of all child processes of the current process
    pub children: Vec<Arc<TaskControlBlock>>,

    /// It is set when active exit or execution error occurs
    pub exit_code: i32,

    /// Heap bottom
    pub heap_bottom: usize,

    /// Program break
    pub program_brk: usize,

    /// stride
    pub stride: usize,

    /// priority
    pub priority: usize,
}

impl TaskControlBlockInner {
    /// get the trap context
    pub fn get_trap_cx(&self) -> &'static mut TrapContext {
        self.trap_cx_ppn.get_mut()
    }
    /// get the user token
    pub fn get_user_token(&self) -> usize {
        self.memory_set.token()
    }
    fn get_status(&self) -> TaskStatus {
        self.task_status
    }
    pub fn is_zombie(&self) -> bool {
        self.get_status() == TaskStatus::Zombie
    }
}

impl TaskControlBlock {
    /// Create a new process
    ///
    /// At present, it is only used for the creation of initproc
    pub fn new(elf_data: &[u8]) -> Self {
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT_BASE).into())
            .unwrap()
            .ppn();

        // alloc a pid and a kernel stack in kernel space
        let pid_handle = pid_alloc();
        let kernel_stack = kstack_alloc();
        let kernel_stack_top = kernel_stack.get_top();
        let task_info = TaskInfo{
            status: TaskStatus::UnInit,
            syscall_times: [0;MAX_SYSCALL_NUM],
            time: 0,
        };
        let timestamp = get_time_ms();
        //debug!("task_info:{:?}",timestamp);
        // push a task context which goes to trap_return to the top of kernel stack
        let task_control_block = Self {
            pid: pid_handle,
            kernel_stack,
            inner: unsafe {
                UPSafeCell::new(TaskControlBlockInner {
                    trap_cx_ppn,
                    base_size: user_sp,
                    task_cx: TaskContext::goto_trap_return(kernel_stack_top),
                    task_status: TaskStatus::Ready,
                    memory_set,
                    parent: None,
                    children: Vec::new(),
                    exit_code: 0,
                    heap_bottom: user_sp,
                    program_brk: user_sp,
                    task_info: task_info,
                    timestamp: timestamp,
                    stride:0,
                    priority:16,
                })
            },
        };
        // prepare TrapContext in user space
        let trap_cx = task_control_block.inner_exclusive_access().get_trap_cx();
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            kernel_stack_top,
            trap_handler as usize,
        );
        task_control_block
    }

    /// Load a new elf to replace the original application address space and start execution
    pub fn exec(&self, elf_data: &[u8]) {
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT_BASE).into())
            .unwrap()
            .ppn();

        // **** access current TCB exclusively
        let mut inner = self.inner_exclusive_access();
        // substitute memory_set
        inner.memory_set = memory_set;
        // update trap_cx ppn
        inner.trap_cx_ppn = trap_cx_ppn;
        // initialize base_size
        inner.base_size = user_sp;
        // initialize trap_cx
        let trap_cx = inner.get_trap_cx();
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            self.kernel_stack.get_top(),
            trap_handler as usize,
        );
        // **** release inner automatically
    }
    /// parent process spawn the child process
    pub fn spawn(self: &Arc<Self>, elf_data: &[u8]) -> Arc<TaskControlBlock> {
        // ---- access parent PCB exclusively
        let mut parent_inner = self.inner_exclusive_access();
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT_BASE).into())
            .unwrap()
            .ppn();
       // alloc a pid and a kernel stack in kernel space
       let pid_handle = pid_alloc();
       let kernel_stack = kstack_alloc();
       let kernel_stack_top = kernel_stack.get_top();
       let task_control_block = Arc::new(TaskControlBlock {
           pid: pid_handle,
           kernel_stack,
           inner: unsafe {
               UPSafeCell::new(TaskControlBlockInner {
                   trap_cx_ppn,
                   base_size: user_sp,
                   task_cx: TaskContext::goto_trap_return(kernel_stack_top),
                   task_status: TaskStatus::Ready,
                   memory_set,
                   parent: Some(Arc::downgrade(self)),
                   children: Vec::new(),
                   exit_code: 0,
                   heap_bottom: parent_inner.heap_bottom,
                   program_brk: parent_inner.program_brk,
                   task_info: parent_inner.task_info,
                   timestamp: parent_inner.timestamp,
                   priority: parent_inner.priority,
                   stride:parent_inner.stride,
               })
           },
       });
       // add child
       parent_inner.children.push(task_control_block.clone());
       // modify kernel_sp in trap_cx
       // **** access child PCB exclusively
       let trap_cx = task_control_block.inner_exclusive_access().get_trap_cx();
  
       // **** release child PCB
       // ---- release parent PCB
        // initialize trap_cx
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            kernel_stack_top,
            trap_handler as usize,
        );
        // return
        task_control_block
    }
    /// parent process fork the child process
    pub fn fork(self: &Arc<Self>) -> Arc<Self> {
        // ---- access parent PCB exclusively
        let mut parent_inner = self.inner_exclusive_access();
        // copy user space(include trap context)
        let memory_set = MemorySet::from_existed_user(&parent_inner.memory_set);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT_BASE).into())
            .unwrap()
            .ppn();
        // alloc a pid and a kernel stack in kernel space
        let pid_handle = pid_alloc();
        let kernel_stack = kstack_alloc();
        let kernel_stack_top = kernel_stack.get_top();
        let task_control_block = Arc::new(TaskControlBlock {
            pid: pid_handle,
            kernel_stack,
            inner: unsafe {
                UPSafeCell::new(TaskControlBlockInner {
                    trap_cx_ppn,
                    base_size: parent_inner.base_size,
                    task_cx: TaskContext::goto_trap_return(kernel_stack_top),
                    task_status: TaskStatus::Ready,
                    memory_set,
                    parent: Some(Arc::downgrade(self)),
                    children: Vec::new(),
                    exit_code: 0,
                    heap_bottom: parent_inner.heap_bottom,
                    program_brk: parent_inner.program_brk,
                    task_info: parent_inner.task_info,
                    timestamp: parent_inner.timestamp,
                    stride:parent_inner.stride,
                    priority:parent_inner.priority,
                })
            },
        });
        // add child
        parent_inner.children.push(task_control_block.clone());
        // modify kernel_sp in trap_cx
        // **** access child PCB exclusively
        let trap_cx = task_control_block.inner_exclusive_access().get_trap_cx();
        trap_cx.kernel_sp = kernel_stack_top;
        // return
        task_control_block
        // **** release child PCB
        // ---- release parent PCB
    }

    /// get pid of process
    pub fn getpid(&self) -> usize {
        self.pid.0
    }

    /// actual impl syscall_mmap, we create map_perm and insert_framed_area for it
    pub fn syscall_mmap(& self, start: usize, len: usize, port: usize) -> isize {
        let mut inner = self.inner_exclusive_access();
        let start_v:VirtAddr= start.into();
        let end_v:VirtAddr = (start+len).into();
        if start_v.floor() >= end_v.ceil(){
            debug!("syscall_mmap start_v:{:?} > end_v:{:?}", start_v, end_v);
            return -1;
        }
        let mut ret:isize = 0;
        if !start_v.aligned()
        {
            error!("syscall_mmap start not algin:{:?}", start_v);
            ret = -1;
        }
        if port & !0x7 != 0 {
            ret = -1;
        }
        if port & 0x7 == 0{
            ret = -1;
        }
        let virt_range = VPNRange::new(start_v.floor(), end_v.ceil());
        for vpn in virt_range{
            debug!("syscall_mmap vpn:{:?}", vpn);
            match  inner.memory_set.translate(vpn){
                Some(pte) => {
                    debug!("syscall_mmap pte{:#x} vaild:{:?}",pte.flags(), pte.is_valid());
                    if pte.is_valid(){ ret = -1;};
                },
                None => {debug!("vpn translate none")},
            };
        }
        if ret == 0{
            let mut map_perm = MapPermission::U;
            if port&0x1 > 0 {
                map_perm |= MapPermission::R;
            }
            if port&0x2 > 0 {
                map_perm |= MapPermission::W;            
            }
            if port&0x4 > 0 {
                map_perm |= MapPermission::W;            
            }
            inner.memory_set.insert_framed_area(start_v, end_v, map_perm);
            
        }
        debug!("syscall_mmap ret:{:?}", ret);   
        ret
    }

    /// actual impl syscall_munmap, we create map_perm and insert_framed_area for it
    pub fn syscall_munmap(& self, start: usize, len: usize) -> isize {
        let mut inner = self.inner_exclusive_access();
        let start_v:VirtAddr= start.into();
        let end_v:VirtAddr = (start+len).into();
        if start_v.floor() >= end_v.ceil(){
            debug!("start_v:{:?} > end_v:{:?}", start_v, end_v);
            return -1;
        }
        if !start_v.aligned()
        {
            error!("syscall_munmap start not algin:{:?}", start_v);
            return -1;
        }
        let virt_range = VPNRange::new(start_v.floor(), end_v.ceil());
    
        for vpn in virt_range{
            debug!("syscall_mmap vpn:{:?}", vpn);
            match  inner.memory_set.translate(vpn){
                Some(pte) => {
                    debug!("syscall_munmap pte{:#x} vaild:{:?}",pte.flags(), pte.is_valid());
                    if !pte.is_valid() {return -1;}
                },
                None => {debug!("syscall_munmap translate none");return -1;},
            };
        };
        debug!("syscall_munmap shrink_to:{:#x}", start_v.0);        
        if inner.memory_set.shrink_to(start_v, start_v){
            for vpn in virt_range{
                debug!("syscall_munmap vpn:{:?}", vpn);
                match  inner.memory_set.translate(vpn){
                    Some(pte) => {
                        debug!("syscall_munmap pte{:#x} vaild:{:?}",pte.flags(), pte.is_valid());
                    },
                    None => {debug!("syscall_munmap translate none");},
                }
            };
            0
        }else{
            -1
        }
    }
    /// get information of task
    pub fn sys_get_task_status(& self) -> TaskInfo {   
        let mut inner = self.inner_exclusive_access();
        inner.task_info.time = inner.timestamp;
        inner.task_info
    }
    /// set syscall counter of task
    pub fn syscall_count(& self, call_id:usize) {   
        let mut inner: RefMut<'_, TaskControlBlockInner> = self.inner_exclusive_access();
        debug!("syscall_times[{}]= {}",call_id, inner.task_info.syscall_times[call_id]);
        inner.task_info.syscall_times[call_id] += 1;
    }
    /// set syscall counter of task
    pub fn syscall_set_priority(& self, priority:usize) {   
        let mut inner: RefMut<'_, TaskControlBlockInner> = self.inner_exclusive_access();
        debug!(" set pid:{} priority from {} into:{}", self.getpid(), inner.priority, priority);
        inner.priority = priority;
    }
     /// set syscall counter of task
     pub fn syscall_set_next_stride(& self) {   
        let mut inner: RefMut<'_, TaskControlBlockInner> = self.inner_exclusive_access();
        inner.stride += MAX_STRIDE_NUM /inner.priority;
    }   
    /// change the location of the program break. return None if failed.
    pub fn change_program_brk(&self, size: i32) -> Option<usize> {
        let mut inner = self.inner_exclusive_access();
        let heap_bottom = inner.heap_bottom;
        let old_break = inner.program_brk;
        let new_brk = inner.program_brk as isize + size as isize;
        if new_brk < heap_bottom as isize {
            return None;
        }
        let result = if size < 0 {
            inner
                .memory_set
                .shrink_to(VirtAddr(heap_bottom), VirtAddr(new_brk as usize))
        } else {
            inner
                .memory_set
                .append_to(VirtAddr(heap_bottom), VirtAddr(new_brk as usize))
        };
        if result {
            inner.program_brk = new_brk as usize;
            Some(old_break)
        } else {
            None
        }
    }

}

#[derive(Copy, Clone, PartialEq)]
/// task status: UnInit, Ready, Running, Exited
pub enum TaskStatus {
    /// uninitialized
    UnInit,
    /// ready to run
    Ready,
    /// running
    Running,
    /// exited
    Zombie,
}
