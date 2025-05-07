# rCore-Camp-2025s ch8报告
[rcore-camp-guide](https://learningos.cn/rCore-Camp-Guide-2025S/chapter8/5exercise.html)说了ch8不需合并之前的代码:

> 本次实验框架变动较大，且改动较为复杂，为降低同学们的工作量，本次实验不要求合并之前的实验内容（除 sys_get_time 以外）， 只需通过 ch8 的全部测例和其他章节的基础测例即可。你可以直接在实验框架的 ch8 分支上完成以下作业。

## 总结功能

用mutex/semaphore当做资源，用银行家算法实现死锁的预防，在用户通过sys_mutex_lock/sys_semaphore_down申请资源时，如果银行家算法检测出来这个need会导致系统不安全，则拒绝线程的这个need，不去尝试分配资源给用户，sys_mutex_lock/sys_semaphore_down直接返回-0xdead。

思路：

首先，用户通过sys_mutex_lock和sys_semaphore_down申请资源，所以一次只会发出对一种资源的需求量为1的请求，sys_mutex_lock和sys_semaphore_down接收到这种需求后，登记这个need（把银行家表need + 1），然后做安全性检查，如果safe就让其通过，不safe就把登记的need撤销，然后返回-0xdead告知线程系统拒绝了这个请求。

注意：对available和allocation的修改是在mutex.lock/unlock、semaphore.up/down真正获取/释放时才有权去修改。银行家算法只起个检查作用，没多少权限。

注意：need不能马上完成（available不够）并不意味着死锁，例如线程A, B和一个mutex，线程A先P(mutex)，然后线程B再P(mutex)，虽然线程B会被卡住，但是此时系统是安全的，因为要认为线程A会释放mutex这种资源，从而B能接着执行下去。

当need不能马上完成，但是这个need经银行家算法检测不会导致系统处于不安全状态时，会接着调用 mutex.lock() / semaphore.down()，线程会卡住，与银行家算法中request > available时线程等待的情况相似。

## ch8的银行家算法与教科书中的银行家算法
这里ch8要写的代码更实际，没有教科书中的银行家算法中不符合实际，不可能提前知晓的max矩阵，need矩阵的概念也有所不同，标准银行家算法中的need是线程还会申请的资源量，而ch8的need矩阵是动态接收到的线程需求(有点像request)。

ch8的银行家算法中，如果接收到一个need，如果available够，肯定会判定算上这个need也处于安全状态，从而马上进行实际的分配(mutex.lock() / semaphore.down())。

证明：如果这个线程能发出一个need，说明其需求已经都被满足了，其need行全为0，否则其应该卡住在lock()/down()中。现在available够，分配给它，其一定能运行完(不考虑其之后还会申请资源，这无法考虑，只看当前情况)，释放其所有资源。由于前一刻是处于安全状态的，于是当前也是处于安全状态。证毕。

而教科书中的银行家算法，发出一个request时，就算available够，其也可能导致系统处于不安全状态。为什么？

因为ch8的银行家算法，need是**运行时动态接收**的，是**以当前的资源申请量判断**各线程能否都运行完，而教科书中的银行家算法是**知晓全局的需求信息**，还需考虑未来的需求，其必须更谨慎才能让系统保持在安全状态。

ch8的need矩阵还有一个特点，由于一次申请只会申请1种资源且量为1，所以need矩阵每行的元素值都<=1，且一行最多一个1(因为如果不能满足需求会lock()卡住，无法再次发出need)。不过可能多行都有1个1，例如A占有2份资源，B，C都请求1份这种资源，need矩阵就有2行都有1。

## 简答作业
### 1. 在我们的多线程实现中，当主线程 (即 0 号线程) 退出时，视为整个进程退出，此时需要结束该进程管理的所有线程并回收其资源。 - 需要回收的资源有哪些？ - 其他线程的 TaskControlBlock 可能在哪些位置被引用，分别是否需要回收，为什么？

ProcessControlBlockInner里的那些资源都需要回收(memory_set、fd_table等)，此外，要把子进程`children: Vec<Arc<ProcessControlBlock>>`挂到`INITPROC`下(代码在os/src/task/mod.rs exit_current_and_run_next())。借助Arc的引用计数机制，可以自动让资源被drop。

其它线程的TaskControlBlock可能在调度队列ready_queue和mutex/semaphore的阻塞队列中存在。

### 2. 对比以下两种 Mutex 中的实现，二者有什么区别？这些区别可能会导致什么问题？

```Rust
impl Mutex for Mutex1 {
    fn lock(&self) {
        loop {
            let mut mutex_inner = self.inner.exclusive_access();
            if mutex_inner.locked {
                mutex_inner.wait_queue.push_back(current_task().unwrap());
                drop(mutex_inner);
                block_current_and_run_next();
            } else {
                mutex_inner.locked = true;
                break;
            }
        }
    }

    fn unlock(&self) {
        let mut mutex_inner = self.inner.exclusive_access();
        assert!(mutex_inner.locked);
        mutex_inner.locked = false;
        if let Some(waking_task) = mutex_inner.wait_queue.pop_front() {
            add_task(waking_task);
        }
    }
}

impl Mutex for Mutex2 {
    fn lock(&self) {
        let mut mutex_inner = self.inner.exclusive_access();
        if mutex_inner.locked {
            mutex_inner.wait_queue.push_back(current_task().unwrap());
            drop(mutex_inner);
            block_current_and_run_next();
        } else {
            mutex_inner.locked = true;
        }
    }

    fn unlock(&self) {
        let mut mutex_inner = self.inner.exclusive_access();
        assert!(mutex_inner.locked);
        if let Some(waking_task) = mutex_inner.wait_queue.pop_front() {
            add_task(waking_task);
        } else {
            mutex_inner.locked = false;
        }
    }
}
```

前置内容：先说`block_current_and_run_next()`与`suspend_current_and_run_next()`，`block_current_and_run_next()`会将当前线程从调度队列ready_queue中拿出，放入相应mutex的阻塞队列，因此线程不会得到调度，得等待mutex唤醒它。而`suspend_current_and_run_next()`只是会进行任务切换，任务仍在ready_queue中，能被再次调度到(也就是说获取锁失败后，仍然可能再被调度)。

`Mutex1`与`os/src/sync/mutex.rs MutexSpin`有点像，但又有所不同。二者都在一个loop循环中获取锁，如果获取锁失败，则每次被唤醒都会重新检查锁状态。对于`Mutex1`，其调用`block_current_and_run_next()`，因此会阻塞，并等到mutex.unlock()唤醒自己时才会再次得到调度机会。对于`MutexSpin`，其调用`suspend_current_and_run_next()`，因此其切换走后仍然有调度机会（也因此`MutexSpin`的`unlock()`中没有唤醒操作），得到调度后会重新检查锁状态并尝试获取。

对于`Mutex1`，由于被从阻塞队列中唤醒的线程只有一个，只要这段时间没有新的ready_queue中的线程尝试获取锁，那么得到锁的线程一定是被`Mutex1.unlock()`唤醒的那一个。其不会与被阻塞的线程重新竞争锁，只会与新来的线程重新竞争。

对于`MutexSpin`，`lock()`失败后会被切换，而不会被阻塞，因此不存在被阻塞的线程。于是重新尝试获取锁时，会与所有想要获取锁的线程竞争。

`Mutex2`是`os/src/sync/mutex.rs MutexBlocking`的代码，当`MutexBlocking.unlock()`时，如果释放锁时mutex的阻塞队列中有线程，则不会修改`mutex_inner.locked`的状态，其仍然为true，并唤醒阻塞队列中的第一个线程，由于其`lock()`没有一个loop，其被唤醒后从`block_current_and_run_next()`中返回后接着执行，`lock()`会返回，其并不会去重新尝试获得锁，而是会认为自己已经获得了锁（这是正确的，`unlock()`保持`mutex_inner.locked`为true就是为它准备的）。

于是，对于`Mutex2`(`MutexBlocking`)，只要阻塞队列中有线程，`unlock()`后一定会是阻塞队列中的第一个线程获得锁。

对于目前rcore单核运行且内核代码不会被打断的前提下，这3种锁的实现我感觉都是对的。

## 荣誉准则
1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与以下各位就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：

    rcore-camp群友

2. 此外，我也参考了以下资料 ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：

    问chatgpt和deepseek相关内容

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。 我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。