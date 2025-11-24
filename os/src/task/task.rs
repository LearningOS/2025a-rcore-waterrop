//! Types related to task management & Functions for completely changing TCB
use super::TaskContext;
use super::{kstack_alloc, pid_alloc, KernelStack, PidHandle};
use crate::config::{PAGE_SIZE, TRAP_CONTEXT_BASE};
use crate::mm::{KERNEL_SPACE, MapPermission, MemorySet, PhysPageNum, VirtAddr, VirtPageNum};
use crate::sync::UPSafeCell;
use crate::trap::{trap_handler, TrapContext};
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::cell::RefMut;

/// Task control block structure
///
/// Directly save the contents that will not change during running
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
    /// 指出了应用地址空间中的 Trap 上下文被放在的物理页帧的物理页号。
    pub trap_cx_ppn: PhysPageNum,

    /// Application data can only appear in areas
    /// where the application address space is lower than base_size
    /// 应用数据仅有可能出现在应用地址空间低于 base_size 字节的区域中。
    /// 借助它我们可以清楚的知道应用有多少数据驻留在内存中。
    pub base_size: usize,

    /// Save task context
    /// 保存任务上下文，用于任务切换。
    pub task_cx: TaskContext,

    /// Maintain the execution status of the current process
    /// 维护当前进程的执行状态。
    pub task_status: TaskStatus,

    /// Application address space
    /// 表示应用地址空间。
    pub memory_set: MemorySet,

    /// Parent process of the current process.
    /// Weak will not affect the reference count of the parent
    /// 指向当前进程的父进程（如果存在的话）。
    /// 使用 Weak 而非 Arc 来包裹另一个任务控制块，因此这个智能指针将不会影响父进程的引用计数。
    pub parent: Option<Weak<TaskControlBlock>>,

    /// A vector containing TCBs of all child processes of the current process
    /// 将当前进程的所有子进程的任务控制块以 Arc 智能指针的形式保存在一个向量中，这样才能够更方便的找到它们。
    pub children: Vec<Arc<TaskControlBlock>>,

    /// It is set when active exit or execution error occurs
    pub exit_code: i32,

    /// Heap bottom
    pub heap_bottom: usize,

    /// Program break
    pub program_brk: usize,

    /// stride
    pub stride: usize,

    /// pass
    pub pass: usize,

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
                    stride: 0,
                    pass: 0,
                    priority: 16
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
                    stride: parent_inner.stride,
                    pass: parent_inner.pass,
                    priority: parent_inner.priority
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

    /// 进行内存映射
    pub fn mmap(&self, start: usize, len: usize, prot: usize) -> isize{
        if start % PAGE_SIZE != 0 { return -1; }        // start 需要映射的虚存起始地址，要求按页对齐
        if len == 0 { return 0; }       // len 映射字节长度，可以为 0
        if (prot & !0x7) != 0 { return -1; }      // prot 其余位必须为0
        if (prot & 0x7) == 0 { return -1; }       // 这样的内存无意义
        let mut inner = self.inner_exclusive_access();
        let mut perm = MapPermission::U;    // 创建权限
        if prot & 0x1 == 0x1 { perm |= MapPermission::R};
        if prot & 0x2 == 0x2 { perm |= MapPermission::W};
        if prot & 0x4 == 0x4 { perm |= MapPermission::X};
        let page_cnt = if len % PAGE_SIZE == 0 {
            len / PAGE_SIZE
        }
        else {
            len / PAGE_SIZE + 1
        };
        let start_va = VirtAddr::from(start);      // 获得虚拟地址与虚拟页号
        let start_vpn = VirtPageNum::from(start_va);
        let end_vpn = VirtPageNum::from(start_vpn.0 + page_cnt);
        let end_va = VirtAddr::from(end_vpn);
        for vpn in (start_vpn.0..end_vpn.0).map(VirtPageNum) {
            if let Some(pte) = inner.memory_set.translate(vpn) {      // 找到虚页号vpn对应的页表项pte
                if pte.is_valid() {
                    println!("{}已经映射了！", vpn.0); 
                    return -1; 
                }        // 
            }
        }
        inner.memory_set.insert_framed_area(start_va, end_va, perm);
        0
    }

    /// 取消内存映射
    pub fn munmap(&self, start: usize, len: usize) -> isize {
        if start % PAGE_SIZE != 0 { return -1; }        // start 需要映射的虚存起始地址，要求按页对齐
        if len == 0 { return 0; }       // len 映射字节长度，可以为 0
        let mut inner = self.inner_exclusive_access();
        let page_cnt = if len % PAGE_SIZE == 0 {
            len / PAGE_SIZE
        }
        else {
            len / PAGE_SIZE + 1
        };
        let start_va = VirtAddr::from(start);      // 获得虚拟地址与虚拟页号
        let start_vpn = VirtPageNum::from(start_va);
        let end_vpn = VirtPageNum::from(start_vpn.0 + page_cnt);
        // let end_va = VirtAddr::from(end_vpn);
        for vpn in (start_vpn.0..end_vpn.0).map(VirtPageNum) {
            if let Some(pte) = inner.memory_set.translate(vpn) {      // 找到虚页号vpn对应的页表项pte
                if !pte.is_valid() { return -1; }        // 
            }
        }
        inner.memory_set.remove_area_with_start_vpn(start_vpn);
        0
    }

    /// 设置优先级
    pub fn set_priority(&self, big_stride: usize, prio: isize) -> isize {
        if prio < 2 { return -1; }
        let mut inner = self.inner_exclusive_access();
        inner.priority = prio as usize;
        inner.pass = big_stride / (prio as usize);
        prio
    }

    /// 获取stride
    pub fn get_stride(&self) -> usize {
        let inner = self.inner_exclusive_access();
        inner.stride
    }

    /// 修改stride
    pub fn update_stride(&self) {
        let mut inner = self.inner_exclusive_access();
        inner.stride += inner.pass;
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
