//! Mutex (spin-like and blocking(sleep))

use super::UPSafeCell;
use crate::task::{current_process, TaskControlBlock};
use crate::task::{block_current_and_run_next, suspend_current_and_run_next};
use crate::task::{current_task, wakeup_task};
use alloc::{collections::VecDeque, sync::Arc};

/// Mutex trait
pub trait Mutex: Sync + Send {
    /// Lock the mutex
    fn lock(&self);
    /// Unlock the mutex
    fn unlock(&self);

    /// 等同于lock()，但是把mutex_id带进来，以修改银行家表
    fn lock_with_mutex_id(&self, mutex_id: usize);

    /// 等同于unlock()，但是把mutex_id带进来，以修改银行家表
    fn unlock_with_mutex_id(&self, mutex_id: usize);
}

/// Spinlock Mutex struct
pub struct MutexSpin {
    locked: UPSafeCell<bool>,
}

impl MutexSpin {
    /// Create a new spinlock mutex
    pub fn new() -> Self {
        Self {
            locked: unsafe { UPSafeCell::new(false) },
        }
    }
}

impl Mutex for MutexSpin {
    /// Lock the spinlock mutex
    fn lock(&self) {
        trace!("kernel: MutexSpin::lock");
        loop {
            let mut locked = self.locked.exclusive_access();
            if *locked {
                drop(locked);
                suspend_current_and_run_next();
                continue;
            } else {
                *locked = true;
                return;
            }
        }
    }

    /// 等同于lock()，但是把mutex_id带进来，以修改银行家表
    fn lock_with_mutex_id(&self, mutex_id: usize) {
        loop {
            let mut locked = self.locked.exclusive_access();
            if *locked {
                drop(locked);
                suspend_current_and_run_next();
                continue;
            } else {
                *locked = true;

                // 自旋锁，获取锁失败后放弃cpu，等重新被调度时继续尝试获取锁，执行到这里说明锁获取成功
                let process = current_process();
                let mut process_inner = process.inner_exclusive_access();
                let tid = current_task().unwrap().inner_exclusive_access().res.as_ref().unwrap().tid;
                process_inner.mutex_banker.add_available(mutex_id, -1);
                process_inner.mutex_banker.add_allocation(tid, mutex_id, 1);
                process_inner.mutex_banker.add_need(tid, mutex_id, -1);
                return;
            }
        }
    }

    fn unlock(&self) {
        trace!("kernel: MutexSpin::unlock");
        let mut locked = self.locked.exclusive_access();
        *locked = false;
    }

    /// 等同于unlock()，但是把mutex_id带进来，以修改银行家表
    fn unlock_with_mutex_id(&self, mutex_id: usize) {
        let mut locked = self.locked.exclusive_access();
        *locked = false;
        let process = current_process();
        let mut process_inner = process.inner_exclusive_access();
        let tid = current_task().unwrap().inner_exclusive_access().res.as_ref().unwrap().tid;
        process_inner.mutex_banker.add_available(mutex_id, 1);
        process_inner.mutex_banker.add_allocation(tid, mutex_id, -1);
    }
}

/// Blocking Mutex struct
pub struct MutexBlocking {
    inner: UPSafeCell<MutexBlockingInner>,
}

pub struct MutexBlockingInner {
    locked: bool,
    wait_queue: VecDeque<Arc<TaskControlBlock>>,
}

impl MutexBlocking {
    /// Create a new blocking mutex
    pub fn new() -> Self {
        trace!("kernel: MutexBlocking::new");
        Self {
            inner: unsafe {
                UPSafeCell::new(MutexBlockingInner {
                    locked: false,
                    wait_queue: VecDeque::new(),
                })
            },
        }
    }
}

impl Mutex for MutexBlocking {
    /// lock the blocking mutex
    fn lock(&self) {
        trace!("kernel: MutexBlocking::lock");
        let mut mutex_inner = self.inner.exclusive_access();
        if mutex_inner.locked {
            mutex_inner.wait_queue.push_back(current_task().unwrap());
            drop(mutex_inner);
            block_current_and_run_next();
        } else {
            mutex_inner.locked = true;
        }
    }

    /// 等同于lock()，但是把mutex_id带进来，以修改银行家表
    fn lock_with_mutex_id(&self, mutex_id: usize) {
        trace!("kernel: MutexBlocking::lock");
        let mut mutex_inner = self.inner.exclusive_access();
        if mutex_inner.locked {
            mutex_inner.wait_queue.push_back(current_task().unwrap());
            drop(mutex_inner);
            // 注意是block_current_and_run_next，会把线程从read_queue中取出，放入锁的等待队列，
            // 其后面不会得到调度，而是被线程unlock锁时唤醒
            block_current_and_run_next();

            // 阻塞锁，执行到这里说明执行unlock的线程直接把锁交给了当前线程，见下面unlock()的注释
            // 相当于先加1再减1，所以available是不变的
            let process = current_process();
            let mut process_inner = process.inner_exclusive_access();
            let tid = current_task().unwrap().inner_exclusive_access().res.as_ref().unwrap().tid;
            process_inner.mutex_banker.add_allocation(tid, mutex_id, 1);
            process_inner.mutex_banker.add_need(tid, mutex_id, -1);
        } else {
            mutex_inner.locked = true;
            // 执行到这里相当于锁空闲，直接就获取到了锁，available要减1
            let process = current_process();
            let mut process_inner = process.inner_exclusive_access();
            let tid = current_task().unwrap().inner_exclusive_access().res.as_ref().unwrap().tid;
            process_inner.mutex_banker.add_available(mutex_id, -1);
            process_inner.mutex_banker.add_allocation(tid, mutex_id, 1);
            process_inner.mutex_banker.add_need(tid, mutex_id, -1);
        }
    }

    /// unlock the blocking mutex
    fn unlock(&self) {
        trace!("kernel: MutexBlocking::unlock");
        let mut mutex_inner = self.inner.exclusive_access();
        assert!(mutex_inner.locked);
        if let Some(waking_task) = mutex_inner.wait_queue.pop_front() {
            // 如果等待队列中有线程，把他从mutex的等待队列中移到read_queue（在mutex的等待队列中时不会被调度运行，
            // 因为block_current_and_run_next()会把当前线程从调度队列ready_queue中移出，放入mutex的阻塞队列中）
            // 而且没有修改mutex_inner.locked，保持为锁住的状态，相当于锁直接从当前线程转移到了被唤醒的线程
            wakeup_task(waking_task);
        } else {
            mutex_inner.locked = false;
        }
    }

    /// 等同于unlock()，但是把mutex_id带进来，以修改银行家表
    fn unlock_with_mutex_id(&self, mutex_id: usize) {
        trace!("kernel: MutexBlocking::unlock");
        let mut mutex_inner = self.inner.exclusive_access();
        assert!(mutex_inner.locked);

        let process = current_process();
        let mut process_inner = process.inner_exclusive_access();
        let tid = current_task().unwrap().inner_exclusive_access().res.as_ref().unwrap().tid;
        if let Some(waking_task) = mutex_inner.wait_queue.pop_front() {
            // 如果等待队列中有线程，把他从mutex的等待队列中移到read_queue（在mutex的等待队列中时不会被调度运行，
            // 因为block_current_and_run_next()会把当前线程从调度队列ready_queue中移出，放入mutex的阻塞队列中）
            // 而且没有修改mutex_inner.locked，保持为锁住的状态，相当于锁直接从当前线程转移到了被唤醒的线程
            // 所以available不变，allocation减1。
            // 也可以这里 available + 1，上面lock_with_mutex_id() block_current_and_run_next()结束从阻塞中唤醒并获得锁时加上 available - 1，最终效果一样
            process_inner.mutex_banker.add_allocation(tid, mutex_id, -1);
            wakeup_task(waking_task);
        } else {
            process_inner.mutex_banker.add_available(mutex_id, 1);
            process_inner.mutex_banker.add_allocation(tid, mutex_id, -1);
            mutex_inner.locked = false;
        }
    }
}
