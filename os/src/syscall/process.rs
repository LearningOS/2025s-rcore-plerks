//! Process management syscalls

use crate::{
    fs::{open_file, OpenFlags},
    mm::{translated_byte_buffer, translated_ref, translated_refmut, translated_str, MapPermission, VirtAddr, VirtPageNum},
    task::{
        add_task, current_task, current_user_token, exit_current_and_run_next, pid2task, suspend_current_and_run_next, SignalAction, SignalFlags, MAX_SIG
    }, timer::get_time_us,
};
use alloc::{string::String, sync::Arc, vec::Vec};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

pub fn sys_exit(exit_code: i32) -> ! {
    trace!("kernel:pid[{}] sys_exit",current_task().unwrap().pid.0);
    exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit!");
}

pub fn sys_yield() -> isize {
    //trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

pub fn sys_getpid() -> isize {
	trace!("kernel: sys_getpid pid:{}", current_task().unwrap().pid.0);
    current_task().unwrap().pid.0 as isize
}

pub fn sys_fork() -> isize {
	trace!("kernel:pid[{}] sys_fork", current_task().unwrap().pid.0);
    let current_task = current_task().unwrap();
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;
    // modify trap context of new_task, because it returns immediately after switching
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    // we do not have to move to next instruction since we have done it before
    // for child process, fork returns 0
    trap_cx.x[10] = 0;
    // add new task to scheduler
    add_task(new_task);
    new_pid as isize
}

pub fn sys_exec(path: *const u8, mut args: *const usize) -> isize {
    trace!("kernel:pid[{}] sys_exec", current_task().unwrap().pid.0);
    let token = current_user_token();
    let path = translated_str(token, path);
    let mut args_vec: Vec<String> = Vec::new();
    loop {
        let arg_str_ptr = *translated_ref(token, args);
        if arg_str_ptr == 0 {
            break;
        }
        args_vec.push(translated_str(token, arg_str_ptr as *const u8));
        unsafe {
            args = args.add(1);
        }
    }
    if let Some(app_inode) = open_file(path.as_str(), OpenFlags::RDONLY) {
        let all_data = app_inode.read_all(); // 把.elf文件的内容一次性全部读到内存中
        let task = current_task().unwrap();
        let argc = args_vec.len();
        task.exec(all_data.as_slice(), args_vec);
        // return argc because cx.x[10] will be covered with it later
        argc as isize
    } else {
        -1
    }
}

/// If there is not a child process whose pid is same as given, return -1.
/// Else if there is a child process but it is still running, return -2.
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
	//trace!("kernel: sys_waitpid");
    let task = current_task().unwrap();
    // find a child process

    // ---- access current PCB exclusively
    let mut inner = task.inner_exclusive_access();
    if !inner
        .children
        .iter()
        .any(|p| pid == -1 || pid as usize == p.getpid())
    {
        return -1;
        // ---- release current PCB
    }
    let pair = inner.children.iter().enumerate().find(|(_, p)| {
        // ++++ temporarily access child PCB exclusively
        p.inner_exclusive_access().is_zombie() && (pid == -1 || pid as usize == p.getpid())
        // ++++ release child PCB
    });
    if let Some((idx, _)) = pair {
        let child = inner.children.remove(idx);
        // confirm that child will be deallocated after being removed from children list
        assert_eq!(Arc::strong_count(&child), 1);
        let found_pid = child.getpid();
        // ++++ temporarily access child PCB exclusively
        let exit_code = child.inner_exclusive_access().exit_code;
        // ++++ release child PCB
        *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code;
        found_pid as isize
    } else {
        -2
    }
    // ---- release current PCB automatically
}

pub fn sys_kill(pid: usize, signum: i32) -> isize {
	trace!("kernel:pid[{}] sys_kill", current_task().unwrap().pid.0);
    if let Some(task) = pid2task(pid) {
        if let Some(flag) = SignalFlags::from_bits(1 << signum) {
            // insert the signal if legal
            let mut task_ref = task.inner_exclusive_access();
            if task_ref.signals.contains(flag) {
                return -1;
            }
            task_ref.signals.insert(flag);
            0
        } else {
            -1
        }
    } else {
        -1
    }
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize { // 在用户程序ch3_sleep.rs中用到
    /* trace!(
        "kernel:pid[{}] sys_get_time NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    ); */
    
    let token = current_user_token();
    let user_address = _ts;
    // buf是TimeVal的物理地址段(由于跨页可能产生分段)，每个段是连续的
    let buf = translated_byte_buffer(token, user_address as *const u8, core::mem::size_of::<TimeVal>());

    let us = get_time_us();
    let time = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };

    // 把time做成u8切片
    let bytes = unsafe {
        // Rust这个类型转换要as两次，不能直接 &T 变成 *const u8，要两步，先 &T 变 *const T，然后 *const T 变 *const u8
        core::slice::from_raw_parts(&time as *const TimeVal as *const u8, core::mem::size_of::<TimeVal>())
    };

    let mut offset = 0;
    for seg in buf {
        let len = seg.len();
        seg.copy_from_slice(&bytes[offset..offset + len]);
        offset += len;
    }

    0
}

use crate::mm::FRAME_ALLOCATOR;

/// YOUR JOB: Implement mmap.
pub fn sys_mmap(_start: usize, _len: usize, _prot: usize) -> isize {
    /* trace!(
        "kernel:pid[{}] sys_mmap NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    ); */
    
    if _prot & !7 != 0 || _prot & 7 == 0 { // 内存页属性检查
        return -1;
    }

    let start_va = VirtAddr::from(_start);
    if !start_va.aligned() { // _start没按页对齐
        return -1;
    }
    
    let start_vpn = start_va.floor();
    let end_va = VirtAddr::from(_start + _len); // 左闭右开，[_start, _start + _len)
    let end_vpn = end_va.ceil();
    // [start_vpn, end_vpn)为要分配的虚拟页框号，左闭右开

    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();

    // 检查[start_vpn, end_vpn)中是否存在已经被分配的页
    for i in start_vpn.0..end_vpn.0 {
        let pte = inner.memory_set.find_pte(VirtPageNum::from(i));

        if let Some(x) = pte {
            if x.is_valid() {
                return -1;
            }
        }
    }

    // 检查物理内存是否足够
    if FRAME_ALLOCATOR.exclusive_access().remain_page_count() < end_vpn.0 - start_vpn.0 {
        return -1;
    }

    // 分配页面
    let mut map_perm = MapPermission::U;
    map_perm |= MapPermission::from_bits((_prot << 1) as u8).unwrap();

    inner.memory_set.insert_framed_area(start_va, end_va, map_perm);

    0
}

/// YOUR JOB: Implement munmap.
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    /* trace!(
        "kernel:pid[{}] sys_munmap NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    ); */
    
    let start_va = VirtAddr::from(_start);
    let start_vpn = start_va.floor();
    let end_va = VirtAddr::from(_start + _len); // 左闭右开，[_start, _start + _len)
    let end_vpn = end_va.ceil();
    
    // [start_vpn, end_vpn)为要unmap的虚拟页框号，左闭右开
    // <https://learningos.cn/rCore-Camp-Guide-2025S/chapter4/7exercise.html#mmap-munmap>:
    // "在 rCore 课程实验中，正确执行的 sys_munmap 仅会对应 唯一且完整 的 mmap 区间，不考虑交叉、截断区间的情况。"
    // 这话的意思应该是，只有给出的要munmap的区间恰在一个MapArea时才能视为正确输入，所以，要检查_start和_end是否
    // 在页边界，以及是否刚好等于某个MapArea包含的页
    
    if !start_va.aligned() || !end_va.aligned() {
        return -1;
    }

    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    let result = inner.memory_set.munmap(start_vpn, end_vpn);
    if result.is_err() {
        return -1;
    }

    0
}

/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel:pid[{}] sys_sbrk", current_task().unwrap().pid.0);
    if let Some(old_brk) = current_task().unwrap().change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}

/// YOUR JOB: Implement spawn.
/// HINT: fork + exec =/= spawn
pub fn sys_spawn(_path: *const u8) -> isize {
    /* trace!(
        "kernel:pid[{}] sys_spawn NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    ); */

    let token = current_user_token();
    let path = translated_str(token, _path);
    if let Some(app_inode) = open_file(path.as_str(), OpenFlags::RDONLY) {
        let task = current_task().unwrap();
        let all_data = app_inode.read_all();
        let new_task = task.spawn(all_data.as_slice());
        let new_pid = new_task.pid.0;
        add_task(new_task);
        new_pid as isize
    } else { // 路径_path不对
        -1
    }
}

// YOUR JOB: Set task priority.
pub fn sys_set_priority(_prio: isize) -> isize {
    /* trace!(
        "kernel:pid[{}] sys_set_priority NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    ); */

    if _prio <= 1 {
        return -1;
    }

    let task = current_task().unwrap();
    task.inner_exclusive_access().priority = _prio as u8;

    return _prio;
}

pub fn sys_sigprocmask(mask: u32) -> isize {
    trace!("kernel:pid[{}] sys_sigprocmask", current_task().unwrap().pid.0);
    if let Some(task) = current_task() {
        let mut inner = task.inner_exclusive_access();
        let old_mask = inner.signal_mask;
        if let Some(flag) = SignalFlags::from_bits(mask) {
            inner.signal_mask = flag;
            old_mask.bits() as isize
        } else {
            -1
        }
    } else {
        -1
    }
}

pub fn sys_sigreturn() -> isize {
    trace!("kernel:pid[{}] sys_sigreturn", current_task().unwrap().pid.0);
    if let Some(task) = current_task() {
        let mut inner = task.inner_exclusive_access();
        inner.handling_sig = -1;
        // restore the trap context
        let trap_ctx = inner.get_trap_cx();
        *trap_ctx = inner.trap_ctx_backup.unwrap();
        // Here we return the value of a0 in the trap_ctx,
        // otherwise it will be overwritten after we trap
        // back to the original execution of the application.
        trap_ctx.x[10] as isize
    } else {
        -1
    }
}

fn check_sigaction_error(signal: SignalFlags, action: usize, old_action: usize) -> bool {
    if action == 0
        || old_action == 0
        || signal == SignalFlags::SIGKILL
        || signal == SignalFlags::SIGSTOP
    {
        true
    } else {
        false
    }
}

pub fn sys_sigaction(
    signum: i32,
    action: *const SignalAction,
    old_action: *mut SignalAction,
) -> isize {
    trace!("kernel:pid[{}] sys_sigaction", current_task().unwrap().pid.0);
    let token = current_user_token();
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if signum as usize > MAX_SIG {
        return -1;
    }
    if let Some(flag) = SignalFlags::from_bits(1 << signum) {
        if check_sigaction_error(flag, action as usize, old_action as usize) {
            return -1;
        }
        let prev_action = inner.signal_actions.table[signum as usize];
        *translated_refmut(token, old_action) = prev_action;
        inner.signal_actions.table[signum as usize] = *translated_ref(token, action);
        0
    } else {
        -1
    }
}
