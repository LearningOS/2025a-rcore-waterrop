//! Process management syscalls
use crate::{
    task::{exit_current_and_run_next, suspend_current_and_run_next},
    timer::get_time_us,
};
use crate::syscall::SYSCALL_COUNTERS;

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(exit_code: i32) -> ! {
    trace!("[kernel] Application exited with code {}", exit_code);
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// get time with second and microsecond
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    unsafe {
        *ts = TimeVal {
            sec: us / 1_000_000,
            usec: us % 1_000_000,
        };
    }
    0
}

// TODO: implement the syscall
/* 
 * 功能:追踪当前任务系统调用的历史信息。
 * 返回值:
 *      _trace_request为0, 返回id地址处的值;
 *      _trace_request为1, 返回0;
 *      _trace_request为2, 返回任务编号为id的系统调用次数
 * syscall ID: 410
 */
pub fn sys_trace(_trace_request: usize, _id: usize, _data: usize) -> isize {
    trace!("kernel: sys_trace");
    unsafe {
        match _trace_request{
            0 => {
                let id_ptr = _id as *const  u8;
                *id_ptr as isize
            }
            1 => {
                let id_ptr = _id as *mut u8;
                *id_ptr = _data as u8;
                0
            }
            2 => {
                if _id < 512 {
                    let counters = SYSCALL_COUNTERS.lock();
                    counters[_id] as isize
                }
                else {-1}
            }
            _ => -1,
        }
    }
}
