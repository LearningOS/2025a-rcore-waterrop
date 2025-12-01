use crate::sync::{Condvar, Mutex, MutexBlocking, MutexSpin, Semaphore};
use crate::task::{block_current_and_run_next, current_process, current_task};
use crate::timer::{add_timer, get_time_ms};
use alloc::sync::Arc;
/// sleep syscall
pub fn sys_sleep(ms: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_sleep",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let expire_ms = get_time_ms() + ms;
    let task = current_task().unwrap();
    add_timer(expire_ms, task);
    block_current_and_run_next();
    0
}
/// mutex create syscall
pub fn sys_mutex_create(blocking: bool) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mutex: Option<Arc<dyn Mutex>> = if !blocking {
        Some(Arc::new(MutexSpin::new()))
    } else {
        Some(Arc::new(MutexBlocking::new()))
    };
    let mut process_inner = process.inner_exclusive_access();
    if let Some(id) = process_inner
        .mutex_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.mutex_list[id] = mutex;
        process_inner.mutex_available[id] = 1 as isize;
        for inner in process_inner.mutex_allocation.iter_mut() {
            inner[id] = 0;
        }
        for inner in process_inner.mutex_need.iter_mut() {
            inner[id] = 0;
        }
        id as isize
    } else {
        process_inner.mutex_list.push(mutex);
        process_inner.mutex_available.push(1 as isize);
        for inner in process_inner.mutex_allocation.iter_mut() {
            inner.push(0);
        }
        for inner in process_inner.mutex_need.iter_mut() {
            inner.push(0);
        }
        process_inner.mutex_list.len() as isize - 1
    }
}
/// mutex lock syscall
pub fn sys_mutex_lock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_lock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    let ena = process_inner.flag;

    drop(process_inner);
    drop(process);
    
    // 只要进来获取锁了，说明该线程需要mutex_id这个资源，则request矩阵对应的资源加1，并更新need
    let task = current_task().unwrap();
    let task_inner = task.inner_exclusive_access();
    let tid = task_inner.res.as_ref().unwrap().tid;
    drop(task_inner);
    drop(task);
    // 线程tid请求mutex_id的资源
    current_process().update_mutex_need(tid, mutex_id, 0);
    let mut flag = true;
    if ena {
        flag = current_process().mutex_is_safe();
    }
    if flag {
        // 线程tid获得了mutex_id的资源
        current_process().update_mutex_allocation(tid, mutex_id, 0);
        // 线程tid已经获得了mutex_id的资源，对应的请求减少
        current_process().update_mutex_need(tid, mutex_id, 1);
        // 分配后，可用资源减少
        current_process().update_mutex_available(mutex_id, 1);
        mutex.lock();
        0
    }
    else { -0xDEAD }
}
/// mutex unlock syscall
pub fn sys_mutex_unlock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_unlock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    drop(process);
    let task = current_task().unwrap();
    let task_inner = task.inner_exclusive_access();
    let tid = task_inner.res.as_ref().unwrap().tid;
    drop(task_inner);
    drop(task);
    current_process().update_mutex_available(mutex_id, 0);
    current_process().update_mutex_allocation(tid, mutex_id, 1);
    mutex.unlock();
    0
}
/// semaphore create syscall
pub fn sys_semaphore_create(res_count: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .semaphore_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.semaphore_list[id] = Some(Arc::new(Semaphore::new(res_count)));
        process_inner.sem_available[id] = res_count as isize;
        for inner in process_inner.sem_allocation.iter_mut() {
            inner[id] = 0;
        }
        for inner in process_inner.sem_need.iter_mut() {
            inner[id] = 0;
        }
        id
    } else {
        process_inner
            .semaphore_list
            .push(Some(Arc::new(Semaphore::new(res_count))));
        process_inner.sem_available.push(res_count as isize);
        for inner in process_inner.sem_allocation.iter_mut() {
            inner.push(0);
        }
        for inner in process_inner.sem_need.iter_mut() {
            inner.push(0);
        }
        process_inner.semaphore_list.len() - 1
    };
    id as isize
}
/// semaphore up syscall
pub fn sys_semaphore_up(sem_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_up",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    drop(process_inner);
    let task = current_task().unwrap();
    let task_inner = task.inner_exclusive_access();
    let tid = task_inner.res.as_ref().unwrap().tid;
    drop(task_inner);
    drop(task);
    current_process().update_sem_available(sem_id, 0);
    current_process().update_sem_allocation(tid, sem_id, 1);
    sem.up();
    0
}
/// semaphore down syscall
pub fn sys_semaphore_down(sem_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_down",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    let ena = process_inner.flag;
    drop(process_inner);

    // 只要进来获取锁了，说明该线程需要mutex_id这个资源，则request矩阵对应的资源加1，并更新need
    let task = current_task().unwrap();
    let task_inner = task.inner_exclusive_access();
    let tid = task_inner.res.as_ref().unwrap().tid;
    drop(task_inner);
    drop(task);
    // 线程tid请求mutex_id的资源
    current_process().update_sem_need(tid, sem_id, 0);
    let mut flag = true;
    if ena {
        flag = current_process().sem_is_safe();
    }
    if flag {
        // 线程tid获得了mutex_id的资源
        current_process().update_sem_allocation(tid, sem_id, 0);
        // 线程tid已经获得了mutex_id的资源，对应的请求减少
        current_process().update_sem_need(tid, sem_id, 1);
        // 分配后，可用资源减少
        current_process().update_sem_available(sem_id, 1);
        sem.down();
        0
    }
    else { -0xDEAD }
}
/// condvar create syscall
pub fn sys_condvar_create() -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .condvar_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.condvar_list[id] = Some(Arc::new(Condvar::new()));
        id
    } else {
        process_inner
            .condvar_list
            .push(Some(Arc::new(Condvar::new())));
        process_inner.condvar_list.len() - 1
    };
    id as isize
}
/// condvar signal syscall
pub fn sys_condvar_signal(condvar_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_signal",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    drop(process_inner);
    condvar.signal();
    0
}
/// condvar wait syscall
pub fn sys_condvar_wait(condvar_id: usize, mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_wait",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    condvar.wait(mutex);
    0
}
/// enable deadlock detection syscall
///
/// YOUR JOB: Implement deadlock detection, but might not all in this syscall
pub fn sys_enable_deadlock_detect(_enabled: usize) -> isize {
    trace!("kernel: sys_enable_deadlock_detect");
    current_process().update_flag(_enabled == 1);
    0
}
