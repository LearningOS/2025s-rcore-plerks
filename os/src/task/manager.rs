//!Implementation of [`TaskManager`]
use super::TaskControlBlock;
use crate::sync::UPSafeCell;
use alloc::collections::VecDeque;
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
    /// 比较两个stride时要化为有符号数进行比较，具体原因为：
    /// 首先，参考[rcore-camp-guide](https://learningos.cn/rCore-Camp-Guide-2025S/chapter5/4exercise.html)，
    /// stride 调度要求进程优先级 >= 2，初始各进程stride值都为0，每次选最小的stride出来，所以系统变化过程中会维持：
    /// STRIDE_MAX – STRIDE_MIN <= BigStride / 2
    /// 我们如何在可能发生溢出的情况下正确比较stride大小？
    /// 有符号数构成一个环 10000000 10000001 ... 00000000 ... 00000001 .. 01111111
    /// 而由STRIDE_MAX – STRIDE_MIN <= BigStride / 2，所有进程的stride范围这个滑窗的长度最大也就BigStride的一半，BigStride可以设为255，
    /// 若将stride保持为无符号数，<的比较在窗口stride值全为负/在0附近(有正有负)/全为正的情况下，都能给出正确结果，
    /// 但是对于像 11111110 11111111 00000000 00000001 的情况，两个进程stride值`{11111110, 11111111}` -> `{(1)00000000, 11111111}`，
    /// 无符号数比较会认为00000000小从而调度运行它，实际上00000000是变大越界的，
    /// 其代表的值其实是 11111111 右边的一个大值，应该运行 11111111
    /// 所以，要把stride值转为有符号比较，各类情况下<比较都能给出正确结果。
    /// 通过这种方式，本来要无限位长来记stride值，现在不用了。
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
            if (stride as i8) < (min_stride as i8) { // 转为有符号数来比较
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
