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
    let mutex_id = if let Some(id) = process_inner
        .mutex_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.mutex_list[id] = mutex;
        id as isize
    } else {
        process_inner.mutex_list.push(mutex);
        process_inner.mutex_list.len() as isize - 1
    };

    // 增加资源种类，也可以写在两类Mutex锁的new里，add_new_resouce会负责银行家表的扩容
    process_inner.mutex_banker.add_new_resouce(mutex_id as usize, 1);
    mutex_id
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
    let mut process_inner = process.inner_exclusive_access();
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    
    let tid = current_task().unwrap().inner_exclusive_access().res.as_ref().unwrap().tid;
    process_inner.mutex_banker.add_need(tid, mutex_id, 1); // need加1
    if process_inner.is_enable {
        // 安全性检查。注意安全性检查只是检查这个need会不会导致不安全，不去修改available和allocation，只是做检查。
        // 真正mutex.lock/unlock成功时（这时真正获取/释放了资源），才去改available和allocation
        let safe = process_inner.mutex_banker.banker_check();
        // debug!("mutex safety check result: {}", safe);
        if !safe {
            process_inner.mutex_banker.add_need(tid, mutex_id, -1); // 不安全，拒绝这个need，返回-0xdead让线程退出
            return -0xdead;
        }
    }

    /* 可以用增加的lock_with_mutex_id()实现lock()，lock_with_mutex_id()里会修改银行家表，更简单的方式是在mutex.lock()返回后
       就可以修改银行家表了。 */

    // 写法一:
    // mutex.lock_with_mutex_id(mutex_id); // 安全性检查放行，让其去获取资源，可能会卡住，但是系统是处于安全状态的

    // 写法二:
    /* lock()前先drop(process_inner)，因为lock()可能会卡住当前线程，这时父函数sys_mutex_lock()这帧还在，process_inner没被drop掉，
       但是其它线程要调sys_mutex_lock()获取process_inner，于是process_inner会被双重借用，发生panic */
    drop(process_inner);
    mutex.lock(); // lock()成功返回后改银行家表，不需要去定义并使用lock_with_mutex_id()
    let mut process_inner = process.inner_exclusive_access();
    process_inner.mutex_banker.add_available(mutex_id, -1);
    process_inner.mutex_banker.add_allocation(tid, mutex_id, 1);
    process_inner.mutex_banker.add_need(tid, mutex_id, -1);

    // drop(process_inner);
    0
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
    let mut process_inner = process.inner_exclusive_access();
    let tid = current_task().unwrap().inner_exclusive_access().res.as_ref().unwrap().tid;
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());

    // 写法一:
    // mutex.unlock_with_mutex_id(mutex_id); // unlock释放资源

    // 写法二:
    mutex.unlock(); // unlock()返回后改银行家表，不需要去定义并使用unlock_with_mutex_id()
    process_inner.mutex_banker.add_available(mutex_id, 1);
    process_inner.mutex_banker.add_allocation(tid, mutex_id, -1);

    drop(process_inner);
    drop(process);
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
    let sem_id = if let Some(id) = process_inner
        .semaphore_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.semaphore_list[id] = Some(Arc::new(Semaphore::new(res_count)));
        id
    } else {
        process_inner
            .semaphore_list
            .push(Some(Arc::new(Semaphore::new(res_count))));
        process_inner.semaphore_list.len() - 1
    };

    // 增加资源种类，也可以写在Semaphore的new里，add_new_resouce会负责银行家表的扩容
    process_inner.semaphore_banker.add_new_resouce(sem_id as usize, res_count);

    sem_id as isize
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
    let mut process_inner = process.inner_exclusive_access();
    let tid = current_task().unwrap().inner_exclusive_access().res.as_ref().unwrap().tid;
    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());

    // 写法一:
    // sem.up_with_sem_id(sem_id); // up释放资源

    // 写法二:
    sem.up(); // up()返回后改银行家表，不需要去定义并使用up_with_sem_id()
    process_inner.semaphore_banker.add_available(sem_id, 1);
    process_inner.semaphore_banker.add_allocation(tid, sem_id, -1);

    drop(process_inner);
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
    let mut process_inner = process.inner_exclusive_access();
    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());

    let tid = current_task().unwrap().inner_exclusive_access().res.as_ref().unwrap().tid;
    process_inner.semaphore_banker.add_need(tid, sem_id, 1);
    if process_inner.is_enable {
        // 安全性检查
        let safe = process_inner.semaphore_banker.banker_check();
        debug!(" ========= ");
        debug!("bank algorithm safety check result: {}", safe);
        debug!("available: {:?}", process_inner.semaphore_banker.available);
        debug!("allocation: {:?}", process_inner.semaphore_banker.allocation);
        debug!("need: {:?}", process_inner.semaphore_banker.need);
        debug!(" ========= ");
        if !safe {
            process_inner.semaphore_banker.add_need(tid, sem_id, -1); // 不安全，拒绝接收这个need
            return -0xdead;
        }
    }

    /* 可以用增加的down_with_sem_id()实现down()，down_with_sem_id()里会修改银行家表，更简单的方式是在sem.down()返回后
       就可以修改银行家表了。 */

    // 写法一:
    // sem.down_with_sem_id(sem_id); // 安全性检查放行，让其去获取资源，可能会卡住，但是系统是处于安全状态的

    // 写法二:
    /* down()前先drop(process_inner)，因为down()可能会卡住当前线程，这时父函数sys_semaphore_down()这帧还在，process_inner没被drop掉，
       但是其它线程要调sys_semaphore_down()获取process_inner，于是process_inner会被双重借用，发生panic */
    drop(process_inner);
    sem.down();
    let mut process_inner = process.inner_exclusive_access();
    process_inner.semaphore_banker.add_available(sem_id, -1);
    process_inner.semaphore_banker.add_allocation(tid, sem_id, 1);
    process_inner.semaphore_banker.add_need(tid, sem_id, -1);

    drop(process_inner);
    0
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
    trace!("kernel: sys_enable_deadlock_detect NOT IMPLEMENTED");
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    process_inner.is_enable = _enabled == 1; // 参数_enabled: 为 1 表示启用死锁检测， 0 表示禁用死锁检测
    0
}
