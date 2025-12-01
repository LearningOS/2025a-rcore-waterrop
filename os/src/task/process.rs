//! Implementation of  [`ProcessControlBlock`]

use super::id::RecycleAllocator;
use super::manager::insert_into_pid2process;
use super::TaskControlBlock;
use super::{add_task, SignalFlags};
use super::{pid_alloc, PidHandle};
use crate::fs::{File, Stdin, Stdout};
use crate::mm::{translated_refmut, MemorySet, KERNEL_SPACE};
use crate::sync::{Condvar, Mutex, Semaphore, UPSafeCell};
use crate::trap::{trap_handler, TrapContext};
use alloc::string::String;
use alloc::sync::{Arc, Weak};
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefMut;

/// Process Control Block
pub struct ProcessControlBlock {
    /// immutable
    pub pid: PidHandle,
    /// mutable
    inner: UPSafeCell<ProcessControlBlockInner>,
}

/// Inner of Process Control Block
pub struct ProcessControlBlockInner {
    /// is zombie?
    pub is_zombie: bool,
    /// memory set(address space)
    pub memory_set: MemorySet,
    /// parent process
    pub parent: Option<Weak<ProcessControlBlock>>,
    /// children process
    pub children: Vec<Arc<ProcessControlBlock>>,
    /// exit code
    pub exit_code: i32,
    /// file descriptor table
    pub fd_table: Vec<Option<Arc<dyn File + Send + Sync>>>,
    /// signal flags
    pub signals: SignalFlags,
    /// tasks(also known as threads)
    pub tasks: Vec<Option<Arc<TaskControlBlock>>>,
    /// task resource allocator
    pub task_res_allocator: RecycleAllocator,
    /// mutex list
    pub mutex_list: Vec<Option<Arc<dyn Mutex>>>,
    /// semaphore list
    pub semaphore_list: Vec<Option<Arc<Semaphore>>>,
    /// condvar list
    pub condvar_list: Vec<Option<Arc<Condvar>>>,
    /// mutex Available
    pub mutex_available: Vec<isize>,
    /// mutex Allocation
    pub mutex_allocation: Vec<Vec<isize>>,
    /// mutex Need
    pub mutex_need: Vec<Vec<isize>>,

    /// sem Available
    pub sem_available: Vec<isize>,
    /// sem Allocation
    pub sem_allocation: Vec<Vec<isize>>,
    /// sem Need
    pub sem_need: Vec<Vec<isize>>,

    /// 是否进行死锁检测
    pub flag: bool,
}

impl ProcessControlBlockInner {
    #[allow(unused)]
    /// get the address of app's page table
    pub fn get_user_token(&self) -> usize {
        self.memory_set.token()
    }
    /// allocate a new file descriptor
    pub fn alloc_fd(&mut self) -> usize {
        if let Some(fd) = (0..self.fd_table.len()).find(|fd| self.fd_table[*fd].is_none()) {
            fd
        } else {
            self.fd_table.push(None);
            self.fd_table.len() - 1
        }
    }
    /// allocate a new task id
    pub fn alloc_tid(&mut self) -> usize {
        self.task_res_allocator.alloc()
    }
    /// deallocate a task id
    pub fn dealloc_tid(&mut self, tid: usize) {
        self.task_res_allocator.dealloc(tid)
    }
    /// the count of tasks(threads) in this process
    pub fn thread_count(&self) -> usize {
        self.tasks.len()
    }
    /// get a task with tid in this process
    pub fn get_task(&self, tid: usize) -> Arc<TaskControlBlock> {
        self.tasks[tid].as_ref().unwrap().clone()
    }
}

impl ProcessControlBlock {
    /// inner_exclusive_access
    pub fn inner_exclusive_access(&self) -> RefMut<'_, ProcessControlBlockInner> {
        self.inner.exclusive_access()
    }
    /// new process from elf file
    pub fn new(elf_data: &[u8]) -> Arc<Self> {
        trace!("kernel: ProcessControlBlock::new");
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (memory_set, ustack_base, entry_point) = MemorySet::from_elf(elf_data);
        // allocate a pid
        let pid_handle = pid_alloc();
        let process = Arc::new(Self {
            pid: pid_handle,
            inner: unsafe {
                UPSafeCell::new(ProcessControlBlockInner {
                    is_zombie: false,
                    memory_set,
                    parent: None,
                    children: Vec::new(),
                    exit_code: 0,
                    fd_table: vec![
                        // 0 -> stdin
                        Some(Arc::new(Stdin)),
                        // 1 -> stdout
                        Some(Arc::new(Stdout)),
                        // 2 -> stderr
                        Some(Arc::new(Stdout)),
                    ],
                    signals: SignalFlags::empty(),
                    tasks: Vec::new(),
                    task_res_allocator: RecycleAllocator::new(),
                    mutex_list: Vec::new(),
                    semaphore_list: Vec::new(),
                    condvar_list: Vec::new(),
                    mutex_available: Vec::new(),
                    mutex_allocation: Vec::new(),
                    mutex_need: Vec::new(),
                    sem_available: Vec::new(),
                    sem_allocation: Vec::new(),
                    sem_need: Vec::new(),
                    flag: false,
                })
            },
        });
        // create a main thread, we should allocate ustack and trap_cx here
        let task = Arc::new(TaskControlBlock::new(
            Arc::clone(&process),
            ustack_base,
            true,
        ));
        // prepare trap_cx of main thread
        let task_inner = task.inner_exclusive_access();
        let trap_cx = task_inner.get_trap_cx();
        let ustack_top = task_inner.res.as_ref().unwrap().ustack_top();
        let kstack_top = task.kstack.get_top();
        drop(task_inner);
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            ustack_top,
            KERNEL_SPACE.exclusive_access().token(),
            kstack_top,
            trap_handler as usize,
        );
        // add main thread to the process
        let mut process_inner = process.inner_exclusive_access();
        process_inner.tasks.push(Some(Arc::clone(&task)));
        drop(process_inner);
        insert_into_pid2process(process.getpid(), Arc::clone(&process));
        // add main thread to scheduler
        add_task(task);
        process
    }

    /// Only support processes with a single thread.
    pub fn exec(self: &Arc<Self>, elf_data: &[u8], args: Vec<String>) {
        trace!("kernel: exec");
        assert_eq!(self.inner_exclusive_access().thread_count(), 1);
        // memory_set with elf program headers/trampoline/trap context/user stack
        trace!("kernel: exec .. MemorySet::from_elf");
        let (memory_set, ustack_base, entry_point) = MemorySet::from_elf(elf_data);
        let new_token = memory_set.token();
        // substitute memory_set
        trace!("kernel: exec .. substitute memory_set");
        self.inner_exclusive_access().memory_set = memory_set;
        // then we alloc user resource for main thread again
        // since memory_set has been changed
        trace!("kernel: exec .. alloc user resource for main thread again");
        let task = self.inner_exclusive_access().get_task(0);
        let mut task_inner = task.inner_exclusive_access();
        task_inner.res.as_mut().unwrap().ustack_base = ustack_base;
        task_inner.res.as_mut().unwrap().alloc_user_res();
        task_inner.trap_cx_ppn = task_inner.res.as_mut().unwrap().trap_cx_ppn();
        // push arguments on user stack
        trace!("kernel: exec .. push arguments on user stack");
        let mut user_sp = task_inner.res.as_mut().unwrap().ustack_top();
        user_sp -= (args.len() + 1) * core::mem::size_of::<usize>();
        let argv_base = user_sp;
        let mut argv: Vec<_> = (0..=args.len())
            .map(|arg| {
                translated_refmut(
                    new_token,
                    (argv_base + arg * core::mem::size_of::<usize>()) as *mut usize,
                )
            })
            .collect();
        *argv[args.len()] = 0;
        for i in 0..args.len() {
            user_sp -= args[i].len() + 1;
            *argv[i] = user_sp;
            let mut p = user_sp;
            for c in args[i].as_bytes() {
                *translated_refmut(new_token, p as *mut u8) = *c;
                p += 1;
            }
            *translated_refmut(new_token, p as *mut u8) = 0;
        }
        // make the user_sp aligned to 8B for k210 platform
        user_sp -= user_sp % core::mem::size_of::<usize>();
        // initialize trap_cx
        trace!("kernel: exec .. initialize trap_cx");
        let mut trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            task.kstack.get_top(),
            trap_handler as usize,
        );
        trap_cx.x[10] = args.len();
        trap_cx.x[11] = argv_base;
        *task_inner.get_trap_cx() = trap_cx;
    }

    /// Only support processes with a single thread.
    pub fn fork(self: &Arc<Self>) -> Arc<Self> {
        trace!("kernel: fork");
        let mut parent = self.inner_exclusive_access();
        assert_eq!(parent.thread_count(), 1);
        // clone parent's memory_set completely including trampoline/ustacks/trap_cxs
        let memory_set = MemorySet::from_existed_user(&parent.memory_set);
        // alloc a pid
        let pid = pid_alloc();
        // copy fd table
        let mut new_fd_table: Vec<Option<Arc<dyn File + Send + Sync>>> = Vec::new();
        for fd in parent.fd_table.iter() {
            if let Some(file) = fd {
                new_fd_table.push(Some(file.clone()));
            } else {
                new_fd_table.push(None);
            }
        }
        // create child process pcb
        let child = Arc::new(Self {
            pid,
            inner: unsafe {
                UPSafeCell::new(ProcessControlBlockInner {
                    is_zombie: false,
                    memory_set,
                    parent: Some(Arc::downgrade(self)),
                    children: Vec::new(),
                    exit_code: 0,
                    fd_table: new_fd_table,
                    signals: SignalFlags::empty(),
                    tasks: Vec::new(),
                    task_res_allocator: RecycleAllocator::new(),
                    mutex_list: Vec::new(),
                    semaphore_list: Vec::new(),
                    condvar_list: Vec::new(),
                    mutex_available: Vec::new(),
                    mutex_allocation: Vec::new(),
                    mutex_need: Vec::new(),
                    sem_available: Vec::new(),
                    sem_allocation: Vec::new(),
                    sem_need: Vec::new(),
                    flag: false,
                })
            },
        });
        // add child
        parent.children.push(Arc::clone(&child));
        // create main thread of child process
        let task = Arc::new(TaskControlBlock::new(
            Arc::clone(&child),
            parent
                .get_task(0)
                .inner_exclusive_access()
                .res
                .as_ref()
                .unwrap()
                .ustack_base(),
            // here we do not allocate trap_cx or ustack again
            // but mention that we allocate a new kstack here
            false,
        ));
        // attach task to child process
        let mut child_inner = child.inner_exclusive_access();
        child_inner.tasks.push(Some(Arc::clone(&task)));
        drop(child_inner);
        // modify kstack_top in trap_cx of this thread
        let task_inner = task.inner_exclusive_access();
        let trap_cx = task_inner.get_trap_cx();
        trap_cx.kernel_sp = task.kstack.get_top();
        drop(task_inner);
        insert_into_pid2process(child.getpid(), Arc::clone(&child));
        // add this thread to scheduler
        add_task(task);
        child
    }
    /// get pid
    pub fn getpid(&self) -> usize {
        self.pid.0
    }
    /// 根据tid找到线程在PCB的tasks里的索引
    pub fn find_task_idx_by_tid(&self, tid:usize) -> Option<usize> {
        let inner = self.inner_exclusive_access();
        // 遍历task列表
        for (idx, opt) in inner.tasks.iter().enumerate() {
            // 跳过空闲位置
            let tcb = match opt {
                Some(x) => x,
                None => continue,
            };
            // 访问tcb的内部数据
            let tcb_inner = tcb.inner_exclusive_access();
            let tcb_user_res = match &tcb_inner.res {
                Some(x) => x,
                None => continue,
            };
            // 判断tid是否相等，若相等则返回idx
            if tcb_user_res.tid == tid {
                return Some(idx);
            }
        }
        // 若遍历完tasks还未找到，则返回None
        None
    }
    /// 更新mutex_avail
    pub fn update_mutex_available(&self, mutex_id: usize, op: usize) {
        // 根据索引找到对应的request的资源列表
        let mut inner = self.inner_exclusive_access();
        let resource = &mut inner.mutex_available;
        if op == 0 {
            resource[mutex_id] += 1;
        }
        else {
            resource[mutex_id] -= 1;
        }
    }
    pub fn update_sem_available(&self, mutex_id: usize, op: usize) {
        // 根据索引找到对应的request的资源列表
        let mut inner = self.inner_exclusive_access();
        let resource = &mut inner.sem_available;
        if op == 0 {
            resource[mutex_id] += 1;
        }
        else {
            resource[mutex_id] -= 1;
        }
    }
    /// 更新mutex_request
    pub fn update_mutex_need(&self, tid: usize, mutex_id: usize, op: usize) {
        // 首先根据tid找到线程在PCB的tasks里的索引
        let idx = self.find_task_idx_by_tid(tid).unwrap();
        // 根据索引找到对应的request的资源列表
        let mut inner = self.inner_exclusive_access();
        let resource = inner.mutex_need.get_mut(idx).unwrap();
        let res = resource.get_mut(mutex_id).unwrap();
        if op == 0 {
            *res += 1;
        }
        else {
            *res -= 1;
        }
    }
    pub fn update_sem_need(&self, tid: usize, mutex_id: usize, op: usize) {
        // 首先根据tid找到线程在PCB的tasks里的索引
        let idx = self.find_task_idx_by_tid(tid).unwrap();
        // 根据索引找到对应的request的资源列表
        let mut inner = self.inner_exclusive_access();
        let resource = inner.sem_need.get_mut(idx).unwrap();
        let res = resource.get_mut(mutex_id).unwrap();
        if op == 0 {
            *res += 1;
        }
        else {
            *res -= 1;
        }
    }
    /// 更新mutex_allocation
        pub fn update_mutex_allocation(&self, tid: usize, mutex_id: usize, op: usize) {
        // 首先根据tid找到线程在PCB的tasks里的索引
        let idx = self.find_task_idx_by_tid(tid).unwrap();
        // 根据索引找到对应的allocation的资源列表
        let mut inner = self.inner_exclusive_access();
        let resource = inner.mutex_allocation.get_mut(idx).unwrap();
        let res = resource.get_mut(mutex_id).unwrap();
        if op == 0 {
            *res += 1;
        }
        else {
            *res -= 1;
        }
    }
    /// 更新sem_allocation
    pub fn update_sem_allocation(&self, tid: usize, mutex_id: usize, op: usize) {
        // 首先根据tid找到线程在PCB的tasks里的索引
        let idx = self.find_task_idx_by_tid(tid).unwrap();
        // 根据索引找到对应的allocation的资源列表
        let mut inner = self.inner_exclusive_access();
        let resource = inner.sem_allocation.get_mut(idx).unwrap();
        let res = resource.get_mut(mutex_id).unwrap();
        if op == 0 {
            *res += 1;
        }
        else {
            *res -= 1;
        }
    }
    /*
    /// 更新need
    /// 调用者确保资源个数一样
    pub fn update_mutex_need(&self, tid: usize, mutex_id: usize) {
        let idx = self.find_task_idx_by_tid(tid).unwrap();
        let mut inner = self.inner_exclusive_access();
        let allocation = inner.mutex_allocation.get(idx).unwrap().clone();
        let request = inner.mutex_request.get(idx).unwrap().clone();
        let need = inner.mutex_need.get_mut(idx).unwrap();
        need[mutex_id] = request[mutex_id] - allocation[mutex_id];
    }
    */
    /// 修改flag
    pub fn update_flag(&self, is_enable: bool) {
        let mut inner = self.inner_exclusive_access();
        inner.flag = is_enable;
    }
    /// 为每个矩阵添加一行
    pub fn update_matrix(&self) {
        let mut inner = self.inner_exclusive_access();
        inner.mutex_allocation.push(Vec::new());
        inner.mutex_need.push(Vec::new());
        inner.sem_allocation.push(Vec::new());
        inner.sem_need.push(Vec::new());
    }
    /// 死锁检测接口
    pub fn mutex_is_safe(&self) -> bool {
        let inner = self.inner_exclusive_access();
        let available = inner.mutex_available.clone();
        let allocation = inner.mutex_allocation.clone();
        let need = inner.mutex_need.clone();
        self.is_safe(available, allocation, need)
    }
    pub fn sem_is_safe(&self) -> bool {
        let inner = self.inner_exclusive_access();
        let available = inner.sem_available.clone();
        let allocation = inner.sem_allocation.clone();
        let need = inner.sem_need.clone();
        self.is_safe(available, allocation, need)
    }
    /// 银行家算法，死锁检测
    fn is_safe(
        &self,
        available: Vec<isize>,
        allocation: Vec<Vec<isize>>,
        need: Vec<Vec<isize>>
    ) -> bool {
        // 检查输入的合法性
        let n_threads = allocation.len();
        if need.len() != n_threads {
            assert!(false, "分配矩阵和需求矩阵线程数不匹配：分配矩阵={}个线程，需求矩阵={}个线程", n_threads, need.len());
            return false;
        }
        // 检查资源数量
        let n_resources = available.len();
        if n_resources == 0 {
            assert!(false, "可利用资源向量为空（无资源类型定义）");
            return false;
        }
        // 校验每个线程的分配/需求资源数与总资源类型数匹配
        for (thread_id, alloc) in allocation.iter().enumerate() {
            if alloc.len() != n_resources {
                assert!(false, "线程{}的分配资源数不匹配：实际{}个，预期{}个", thread_id, alloc.len(), n_resources);
                return false;
            }
        }
        for (thread_id, nd) in need.iter().enumerate() {
            if nd.len() != n_resources {
                assert!(false, "线程{}的需求资源数不匹配：实际{}个，预期{}个", thread_id, nd.len(), n_resources);
                return false;
            }
        }
        // 初始化工作向量Work和结束向量Finish
        let mut work = available.to_vec();
        let mut finish = Vec::new();
        for _i in 0..n_threads { finish.push(false); }
        loop {
            // 查找满足条件的线程：未完成 + 需求<=当前可用资源
            let found_thread = finish
                .iter()
                .enumerate()
                .find(|(thread_id, &is_finished)| {
                    if is_finished { return false; }
                    need[*thread_id]
                        .iter()
                        .zip(work.iter())
                        .all(|(need, work_j)| need <= work_j)
                });
            match found_thread {
                Some((thread_id, _)) => {
                    for id in 0..n_resources {
                        work[id] += allocation[thread_id][id];
                    }
                    finish[thread_id] = true;
                }
                None => {
                    break;
                }
            }
        }
        finish.iter().all(|&is_finished| is_finished)
    }
}
