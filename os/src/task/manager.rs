//! Implementation of [`TaskManager`]
//!
//! It is only used to manage processes and schedule process based on ready queue.
//! Other CPU process monitoring functions are in Processor.

use super::TaskControlBlock;
use crate::sync::UPSafeCell;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::sync::Arc;
use lazy_static::*;
///A array of `TaskControlBlock` that is thread-safe
/// <https://learningos.cn/rCore-Camp-Guide-2025S/chapter5/2core-data-structures.html#id8>
/// TaskManager不再管控制流，只存储等待运行的任务队列，可以add/fetch要运行的任务
pub struct TaskManager {
    ready_queue: VecDeque<Arc<TaskControlBlock>>,
}

const BIG_STRIDE: u8 = 255;

/// A simple FIFO scheduler.
impl TaskManager {
    ///Creat an empty TaskManager
    pub fn new() -> Self {
        Self {
            ready_queue: VecDeque::new(),
        }
    }
    /// Add process back to ready queue
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        let mut inner = task.as_ref().inner_exclusive_access();
        inner.stride += BIG_STRIDE / inner.priority;
        drop(inner);
        self.ready_queue.push_back(task);
    }

    /// Take a process out of the ready queue
    /// 我们要比较两个stride值的大小，但是stride值随着不断+pass，是有可能溢出的，如何处理？
    /// 首先，参考[rcore-camp-guide](https://learningos.cn/rCore-Camp-Guide-2025S/chapter5/4exercise.html)，
    /// stride 调度要求进程优先级 >= 2，初始各进程stride值都为0，每次选最小的stride出来，加上步进值pass = BigStride / priority，
    /// 所以系统变化过程中会维持：
    /// STRIDE_MAX – STRIDE_MIN <= BigStride / 2
    /// 在没有发生溢出的情况下，我们是能正确比较stride值的大小的，但是问题在于进程运行时间久了stride值可能溢出，
    /// 例如当两个进程stride值`{1111_1110, 1111_1111}` -> `{(1)0000_0000, 1111_1111}`时(第一个stride值加了pass 2)，
    /// 无符号数比较会认为0000_0000小从而调度运行它，实际上0000_0000是变大越界的数。
    /// 把stride值当成有符号数来比较也是不行的，虽然上面的边界情况能处理，但是对于`{0111_1110, 0111_1111}` -> `{1000_0000, 0111_1111}`，
    /// 会继续选择1000_0000出来运行。
    /// 
    /// 那么，怎样的策略才能使得即便考虑溢出，我们也能正确进行stride值的比较？
    /// 
    /// 机器数构成一个环 0000_0000 ... 0111_1111 1000_0000 ... 1111_1111
    /// 在环上，stride值都是顺时针在跑（stride值增大），只要不超过一整圈，两个stride值a, b相减就是他们之间的距离（可正可负），
    /// （一圈是指例如stride是u8，所有u8的数构成一个环）
    /// 于是我们考虑 a - b，把 a - b 转成有符号数，判断正负即可知道到底是a在前面还是b在前面，(也可以不转有符号数，直接判断最高位是0/1)
    /// 但是这里还有个问题，a - b 不能溢出符号位，否则判断 a - b 的正负会有问题。也就是说a不能在b前面太多，a如果在b前面1000_0000步，
    /// 我们会误认为这是个负数，会误以为a在b后面。
    /// 而由STRIDE_MAX – STRIDE_MIN <= BigStride / 2，BigStride可以设为255，即保证了 a - b 不会溢出符号位，
    /// 也保证了不会超过一整圈。
    /// 为什么BigStride要取上界？
    /// 不然的话有效的priority取值少，例如BigStride是3，那么不管priority怎么取，BigStride / priority只会是 1 或者 0
    /// 
    /// 总结一句话：算 a - b 的正负，且要控制差值不能溢出符号位。
    /// 
    /// 通过判断 (signed)(a - b) 的正负，本来要无限位长来记stride值，现在不用了。
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        if self.ready_queue.is_empty() {
            return None;
        }
        let mut index = 0;
        let mut min_stride = self.ready_queue[0].as_ref().inner_exclusive_access().stride;
        // 不用优先队列，直接暴力遍历，[rcore-camp-guide](https://learningos.cn/rCore-Camp-Guide-2025S/chapter5/4exercise.html):
        // stride 算法要找到 stride 最小的进程，使用优先级队列是效率不错的办法，但是我们的实验测例很简单，所以效率完全不是问题。事实上，很推荐使用暴力扫一遍的办法找最小值。
        for i in 1..self.ready_queue.len() {
            let stride = self.ready_queue[0].as_ref().inner_exclusive_access().stride;
            if ((stride - min_stride) as i8) < 0 { // 判断(signed)(a - b) 的正负
                index = i;
                min_stride = stride;
            }
        }
        self.ready_queue.remove(index)
    }
}

lazy_static! {
    /// TASK_MANAGER instance through lazy_static!
    pub static ref TASK_MANAGER: UPSafeCell<TaskManager> =
        unsafe { UPSafeCell::new(TaskManager::new()) };
    /// PID2PCB instance (map of pid to pcb)
    pub static ref PID2TCB: UPSafeCell<BTreeMap<usize, Arc<TaskControlBlock>>> =
        unsafe { UPSafeCell::new(BTreeMap::new()) };
}

/// Add process to ready queue
pub fn add_task(task: Arc<TaskControlBlock>) {
	//trace!("kernel: TaskManager::add_task");
    PID2TCB
        .exclusive_access()
        .insert(task.getpid(), Arc::clone(&task));
    TASK_MANAGER.exclusive_access().add(task);
}

/// Take a process out of the ready queue
pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
	//trace!("kernel: TaskManager::fetch_task");
    TASK_MANAGER.exclusive_access().fetch()
}

/// Get process by pid
pub fn pid2task(pid: usize) -> Option<Arc<TaskControlBlock>> {
    let map = PID2TCB.exclusive_access();
    map.get(&pid).map(Arc::clone)
}

/// Remove item(pid, _some_pcb) from PDI2PCB map (called by exit_current_and_run_next)
pub fn remove_from_pid2task(pid: usize) {
    let mut map = PID2TCB.exclusive_access();
    if map.remove(&pid).is_none() {
        panic!("cannot find pid {} in pid2task!", pid);
    }
}
