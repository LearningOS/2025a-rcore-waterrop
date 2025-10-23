//! Process management syscalls

use crate::{mm::{translated_refmut, validate_user_addr}, task::{change_program_brk, current_user_token, exit_current_and_run_next, get_syscall_count, suspend_current_and_run_next, insert_mmap}, timer::{get_time, get_time_us}};

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
    let current_time_us = get_time_us();
    let current_time_s = get_time();
    let token = current_user_token();
    if let Some(ts) = translated_refmut::<TimeVal>(token, _ts as *const TimeVal) {
        unsafe {
            (*ts).usec = current_time_us;
            (*ts).sec = current_time_s;
            0
        }
    }
    else {
        -1
    }
    
}

/// TODO: Finish sys_trace to pass testcases
/// HINT: You might reimplement it with virtual memory management.
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
        let user_token = current_user_token();
        match _trace_request{
            0 => {
                print!("_trace_request0\n");
                let flag = validate_user_addr(user_token, _id, false);
                if flag {
                    if let Some(phys_ptr) = translated_refmut::<u8>(user_token, _id as *const  u8) {
                        (*phys_ptr) as isize
                    }
                    else {
                        -1 as isize
                    }
                }
                else { -1 as isize }
                
            }
            1 => {
                print!("_trace_request1\n");
                let flag = validate_user_addr(user_token, _id, true);
                if flag {
                    if let Some(phys_ptr) = translated_refmut::<u8>(user_token, _id as *const u8){
                        (*phys_ptr) = _data as u8;
                        0
                    }
                    else {
                        -1 as isize
                    }
                }
                else {
                    -1 as isize
                }
            }
            2 => {
                print!("_trace_request2\n");
                if _id < 512 {
                    let count = get_syscall_count(_id) as isize;
                    print!("{}count:{}\n", _id, count);
                    count
                }
                else {-1}
            }
            _ => -1,
        }
    }
}

// YOUR JOB: Implement mmap.
/// 申请长度为 len 字节的物理内存（不要求实际物理内存位置，可以随便找一块），将其映射到 start 开始的虚存，内存页属性为 prot
/// start 需要映射的虚存起始地址，要求按页对齐
/// len 映射字节长度，可以为 0
/// len 映射字节长度，可以为 0
/// 返回值：执行成功则返回 0，错误返回 -1
pub fn sys_mmap(_start: usize, _len: usize, _prot: usize) -> isize {
    trace!("kernel: sys_mmap NOT IMPLEMENTED YET!");
    insert_mmap(_start, _len, _prot)
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    trace!("kernel: sys_munmap NOT IMPLEMENTED YET!");
    -1
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
