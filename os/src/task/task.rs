//! Types related to task management
//use riscv::paging::PTE;

use super::TaskContext;

use crate::config::TRAP_CONTEXT_BASE;
use crate::mm::{
    kernel_stack_position, MapPermission, MemorySet, PhysPageNum, VirtAddr, KERNEL_SPACE,VPNRange,
};
use crate::trap::{trap_handler, TrapContext};
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

    /// The phys page number of trap context
    pub trap_cx_ppn: PhysPageNum,

    /// The size(top addr) of program which is loaded from elf file
    pub base_size: usize,

    /// Heap bottom
    pub heap_bottom: usize,

    /// Program break
    pub program_brk: usize,
}

impl TaskControlBlock {
    /// get the trap context
    pub fn get_trap_cx(&self) -> &'static mut TrapContext {
        self.trap_cx_ppn.get_mut()
    }
    /// get the user token
    pub fn get_user_token(&self) -> usize {
        self.memory_set.token()
    }
    /// Based on the elf info in program, build the contents of task in a new address space
    pub fn new(elf_data: &[u8], app_id: usize) -> Self {
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT_BASE).into())
            .unwrap()
            .ppn();
        let task_status = TaskStatus::Ready;
        // map a kernel-stack in kernel space
        let (kernel_stack_bottom, kernel_stack_top) = kernel_stack_position(app_id);
        KERNEL_SPACE.exclusive_access().insert_framed_area(
            kernel_stack_bottom.into(),
            kernel_stack_top.into(),
            MapPermission::R | MapPermission::W,
        );
        let task_info = TaskInfo{
            status: TaskStatus::UnInit,
            syscall_times: [0;MAX_SYSCALL_NUM],
            time: 0,
        };
        let timestamp = get_time_ms();
        //debug!("task_info:{:?}",timestamp);
        let task_control_block = Self {
            task_status,
            task_cx: TaskContext::goto_trap_return(kernel_stack_top),
            memory_set,
            trap_cx_ppn,
            base_size: user_sp,
            heap_bottom: user_sp,
            program_brk: user_sp,
            task_info: task_info,
            timestamp: timestamp,
        };
        // prepare TrapContext in user space
        let trap_cx = task_control_block.get_trap_cx();
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            kernel_stack_top,
            trap_handler as usize,
        );
        task_control_block
    }
    /// actual impl syscall_mmap, we create map_perm and insert_framed_area for it
    pub fn syscall_mmap(&mut self, start: usize, len: usize, port: usize) -> isize {
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
            match  self.memory_set.translate(vpn){
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
            self.memory_set.insert_framed_area(start_v, end_v, map_perm);
            
        }
        debug!("syscall_mmap ret:{:?}", ret);   
        ret
    }

    /// actual impl syscall_munmap, we create map_perm and insert_framed_area for it
    pub fn syscall_munmap(&mut self, start: usize, len: usize) -> isize {
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
            match  self.memory_set.translate(vpn){
                Some(pte) => {
                    debug!("syscall_munmap pte{:#x} vaild:{:?}",pte.flags(), pte.is_valid());
                    if !pte.is_valid() {return -1;}
                },
                None => {debug!("syscall_munmap translate none");return -1;},
            };
        };
        debug!("syscall_munmap shrink_to:{:#x}", start_v.0);        
        if self.memory_set.shrink_to(start_v, start_v){
            for vpn in virt_range{
                debug!("syscall_munmap vpn:{:?}", vpn);
                match  self.memory_set.translate(vpn){
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
    /// change the location of the program break. return None if failed.
    pub fn change_program_brk(&mut self, size: i32) -> Option<usize> {
        let old_break = self.program_brk;
        let new_brk = self.program_brk as isize + size as isize;
        if new_brk < self.heap_bottom as isize {
            return None;
        }
        let result = if size < 0 {
            self.memory_set
                .shrink_to(VirtAddr(self.heap_bottom), VirtAddr(new_brk as usize))
        } else {
            self.memory_set
                .append_to(VirtAddr(self.heap_bottom), VirtAddr(new_brk as usize))
        };
        if result {
            self.program_brk = new_brk as usize;
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
    Exited,
}
