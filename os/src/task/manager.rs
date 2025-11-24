//!Implementation of [`TaskManager`]

use super::TaskControlBlock;
use crate::sync::UPSafeCell;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use crate::config::{BIG_STRIDE};
use lazy_static::*;
///A array of `TaskControlBlock` that is thread-safe
pub struct TaskManager {
    ready_queue: VecDeque<Arc<TaskControlBlock>>,
    big_stride: usize,
}

/// A simple FIFO scheduler.
impl TaskManager {
    ///Creat an empty TaskManager
    pub fn new() -> Self {
        Self {
            ready_queue: VecDeque::new(),
            big_stride: BIG_STRIDE,
        }
    }
    /// Add process back to ready queue
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        self.ready_queue.push_back(task);
    }
    /// Take a process out of the ready queue
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        // 1. 遍历队列，找到 stride 最小的任务的索引
        //    如果队列为空，min_index 会是 None
        let min_index = self.ready_queue
            .iter()
            .enumerate() // 同时获取索引和元素引用
            .min_by_key(|&(_, tcb)| tcb.get_stride()) // 按 stride 值排序，取最小的
            .map(|(index, _)| index); // 只保留索引

        // 2. 根据索引移除并返回任务
        min_index.map(|index| {
            let result_tcb = self.ready_queue.remove(index).unwrap();
            result_tcb.update_stride();
            result_tcb
        })
        // self.ready_queue.pop_front()
    }
    /// 获取big_stride
    pub fn get_big_stride(&self) -> usize {
        self.big_stride
    }
}

lazy_static! {
    /// TASK_MANAGER instance through lazy_static!
    pub static ref TASK_MANAGER: UPSafeCell<TaskManager> =
        unsafe { UPSafeCell::new(TaskManager::new()) };
}

/// Add process to ready queue
pub fn add_task(task: Arc<TaskControlBlock>) {
    //trace!("kernel: TaskManager::add_task");
    TASK_MANAGER.exclusive_access().add(task);
}

/// Take a process out of the ready queue
pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
    //trace!("kernel: TaskManager::fetch_task");
    TASK_MANAGER.exclusive_access().fetch()
}

/// 获取最大stride
pub fn get_big_stride() -> usize {
    TASK_MANAGER.exclusive_access().get_big_stride()
}