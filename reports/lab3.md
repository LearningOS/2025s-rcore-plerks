# rCore-Camp-2025s ch5报告

## 总结功能
1. 迁移 ch4 的 sys_get_time sys_mmap sys_munmap

2. 实现 spawn 系统调用

3. 实现 stride 调度

思路：

1. 迁移 ch4 的 sys_get_time sys_mmap sys_munmap

    函数的实现代码和ch4基本一致，只是ch4的TaskManager在ch5中拆分为了TaskManager和Processor。

2. 实现 spawn 系统调用

    结合TaskControlBlock的fork和exec方法，从中选出直接spawn应该如何构造TaskContext和TrapContext。

3. 实现 [stride 调度](https://learningos.cn/rCore-Camp-Guide-2025S/chapter5/4exercise.html#stride)

    在TaskControlBlockInner中增加字段`stride: u8`和`priority: u8`。然后在`TaskManager.fetch()`时暴力遍历ready_queue，选出stride值最小的TaskControlBlock（可用优先队列优化，但指导书推荐直接暴力枚举即可，实验测例很简单，不会有性能问题），在`TaskManager.add()`时更新stride值，加上步进值`BIG_STRIDE / priority`。

## 简答作业
stride 算法深入

stride 算法原理非常简单，但是有一个比较大的问题。例如两个 pass = 10 的进程，使用 8bit 无符号整形储存 stride， p1.stride = 255, p2.stride = 250，在 p2 执行一个时间片后，理论上下一次应该 p1 执行。

* 实际情况是轮到 p1 执行吗？为什么？

    否，250加上步进值10，无符号数溢出了，会变成一个较小的正数，这时会认为 (250 + 10) mod 256 < 255，反而会去执行p2。

我们之前要求进程优先级 >= 2 其实就是为了解决这个问题。可以证明， 在不考虑溢出的情况下 , 在进程优先级全部 >= 2 的情况下，如果严格按照算法执行，那么 STRIDE_MAX – STRIDE_MIN <= BigStride / 2。

* 为什么？尝试简单说明（不要求严格证明）。

    stride 调度要求进程优先级 >= 2，所以步进值 pass 最大不超过BigStride / 2。初始各进程stride值都为0，每次选最小的stride出来，所以系统变化过程中会维持住：
    STRIDE_MAX – STRIDE_MIN <= BigStride / 2。

    这相当于一个双指针，归纳来证明，假设初始状态有STRIDE_MAX – STRIDE_MIN <= BigStride / 2，那么当前有 l <= r，由于步进值pass的上界，步进一下之后 r - l 仍然 <= BigStride / 2。

已知以上结论，考虑溢出的情况下，可以为 Stride 设计特别的比较器，让`BinaryHeap<Stride>`的 pop 方法能返回真正最小的 Stride。补全下列代码中的 partial_cmp 函数，假设两个 Stride 永远不会相等。

```Rust
use core::cmp::Ordering;

struct Stride(u64);

impl PartialOrd for Stride {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        // ...
    }
}

impl PartialEq for Stride {
    fn eq(&self, other: &Self) -> bool {
        false
    }
}
```

TIPS: 使用 8 bits 存储 stride, BigStride = 255, 则: (125 < 255) == false, (129 < 255) == true.

没有定义Stride类型，然后impl PartialOrd。直接把stride值转成有符号数来比较的。[这篇文章](https://blog.csdn.net/u012750235/article/details/131884423)有比较详细的解释。

在TaskManager.fetch()中，用`if (stride as i8) < (min_stride as i8)`来比较stride值。

来捋这个技巧:

---

比较两个stride时要化为有符号数进行比较，具体原因为：

首先，参考[rcore-camp-guide](https://learningos.cn/rCore-Camp-Guide-2025S/chapter5/4exercise.html)，
stride 调度要求进程优先级 >= 2，初始各进程stride值都为0，每次选最小的stride出来，所以系统变化过程中会维持：

STRIDE_MAX – STRIDE_MIN <= BigStride / 2

我们如何在可能发生溢出的情况下正确比较stride大小？

有符号数构成一个环 10000000 10000001 ... 00000000 ... 00000001 .. 01111111

而由STRIDE_MAX – STRIDE_MIN <= BigStride / 2，所有进程的stride范围这个滑窗的长度最大也就BigStride的一半，BigStride可以设为255，

若将stride保持为无符号数，<的比较在窗口stride值全为负/在0附近(有正有负)/全为正的情况下，都能给出正确结果，

但是对于像 11111110 11111111 00000000 00000001 的情况，两个进程stride值`{11111110, 11111111}` -> `{(1)00000000, 11111111}`，无符号数比较会认为00000000小从而调度运行它，实际上00000000是变大越界的，其代表的值其实是 11111111 右边的一个大值，应该运行 11111111

所以，要把stride值转为有符号比较，各类情况下<比较都能给出正确结果。

通过这种方式，本来要无限位长来记stride值，现在不用了。

---

## 荣誉准则
1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与以下各位就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：

    rcore-camp群友

2. 此外，我也参考了以下资料 ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：

    问chatgpt和deepseek相关内容

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。 我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。