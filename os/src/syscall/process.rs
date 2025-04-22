//! Process management syscalls
use crate::{mm::{translated_byte_buffer, VirtAddr, VirtPageNum}, task::{change_program_brk, current_user_token, exit_current_and_run_next, suspend_current_and_run_next, TASK_MANAGER}, timer::get_time_us};

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
/// 内核拿到了TimeVal的指针，但是这个指针地址是用户虚拟空间，所以内核要处理，
/// 这个TimeVal可能是跨页的，但是translated_byte_buffer能正确处理跨页
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");

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

/// TODO: Finish sys_trace to pass testcases
/// HINT: You might reimplement it with virtual memory management.
/// 页表项定义: <https://rcore-os.cn/rCore-Tutorial-Book-v3/chapter4/3sv39-implementation-1.html#id5>
pub fn sys_trace(_trace_request: usize, _id: usize, _data: usize) -> isize { // 这里几个参数都是直接寄存器传过来的，不会有跨页的问题
    trace!("kernel: sys_trace");

    if _trace_request == 0 {
        let pte = TASK_MANAGER.find_pte_by_virtual_address(_id);
        /* 参考<https://zhuanlan.zhihu.com/p/626899526>，
        sv39，虚拟内存地址为64位，但仅低39位有效。一级页号9位 二级页号9位 三级页号9位 页内偏移12位，cpu只会看低39位，
        然后根据satp寄存器的ppn字段拿到一级页表所在的物理页号，然后地址转换。
        VirtAddr::from()里，直接通过位运算把[63,39]变成0了，所以即使这里_id是isize::MAX(没有映射页面)，find_pte也能找到结果。
        但是isize::MAX这个地址非法(超出了39位)，我们需要在这里手动检测。
        */
        if _id > ((1 << 39) - 1) {
            return -1;
        }
        if let Some(x) = pte {
            // debug!("pte: {:b}", pte.unwrap().bits);
            if !x.is_valid() || !x.readable() { // 有pte但不能读
                return -1;
            }
        }
        else { // 没有pte
            return -1;
        }
        let token = current_user_token();
        let user_address = _id as usize;
        let buf = translated_byte_buffer(token, user_address as *const u8, 1);
        let x = buf[0][0];
        return x as isize;
    }
    else if _trace_request == 1 {
        if _id > ((1 << 39) - 1) {
            return -1;
        }
        let pte = TASK_MANAGER.find_pte_by_virtual_address(_id);
        if let Some(x) = pte {
            if !x.is_valid() || !x.writable() {
                return -1;
            }
        }
        else {
            return -1;
        }
        let token = current_user_token();
        let user_address = _id as usize;
        let mut buf = translated_byte_buffer(token, user_address as *const u8, 1);
        buf[0][0] = _data as u8;
        return 0;
    }
    else if _trace_request == 2 {
        return TASK_MANAGER.get_syscall_count(_id) as isize;
    }

    -1
}

use crate::mm::FRAME_ALLOCATOR;

// YOUR JOB: Implement mmap.
pub fn sys_mmap(_start: usize, _len: usize, _prot: usize) -> isize {
    // trace!("kernel: sys_mmap NOT IMPLEMENTED YET!");

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

    // 检查[start_vpn, end_vpn)中是否存在已经被分配的页
    for i in start_vpn.0..end_vpn.0 {
        // debug!("i: {}", i);
        let pte = TASK_MANAGER.find_pte(VirtPageNum::from(i));
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
    TASK_MANAGER.mmap(start_va, end_va, _prot);

    0
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    // trace!("kernel: sys_munmap NOT IMPLEMENTED YET!");

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

    let result = TASK_MANAGER.munmap(start_vpn, end_vpn);
    if result.is_err() {
        return -1;
    }

    0
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
