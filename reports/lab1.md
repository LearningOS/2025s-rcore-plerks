# rCore-Camp-2025s ch3报告

## 总结功能
实现一个新的系统调用
```Rust
fn sys_trace(_trace_request: usize, _id: usize, _data: usize) -> isize
```
根据_trace_request的值完成不同功能：

1. 读取_id地址处的值

2. 将_data写入_id地址处

3. 查询当前任务调用编号为_id的系统调用的次数

思路：

在 os/src/task/mod.rs 的 TASK_MANAGER 中增加一个用作 map 的二维数组: (task号, (syscall_id, 调用次数))。os/src/syscall/mod.rs 中的 syscall() 是根据系统调用号完成系统调用的函数，在 syscall() 中增加代码，将调用计数到 TASK_MANAGER 的记录数组中即可。然后 os/src/syscall/process.rs 中的 sys_trace() 实现只需读取记录数组即可获得系统调用次数（通过TASK_MANAGER公开的读取函数）。

## 简答作业
### 1. 正确进入 U 态后，程序的特征还应有：使用 S 态特权指令，访问 S 态寄存器后会报错。 请同学们可以自行测试这些内容（运行 三个 bad 测例 (ch2b_bad_*.rs) ）， 描述程序出错行为，同时注意注明你使用的 sbi 及其版本。

运行3个bad测例，会触发trap，然后 os/src/trap 中 trap_handler() 会运行下一个用户程序 (通过调用exit_current_and_run_next())：
```Shell
[kernel] PageFault in application, bad addr = 0x0, bad instruction = 0x804003a4, kernel killed it.
[kernel] IllegalInstruction in application, kernel killed it.
[kernel] IllegalInstruction in application, kernel killed it.
```
关于触发trap的方式，第一个是因为尝试写0地址，第二三个是因为指令权限不够，用户程序运行在U-mode，而sret和csrr指令需要S-mode的权限才能运行。

不过这里关于为什么写0地址会触发trap，不太清楚。当前ch3应该都是物理地址，写0地址触发trap，是由谁来检测和触发trap的？目前对此还很模糊。

### 2. 深入理解 trap.S 中两个函数 __alltraps 和 __restore 的作用，并回答如下问题:

#### 1. L40：刚进入 __restore 时，sp 代表了什么值。请指出 __restore 的两种使用情景。

刚进入__restore时，sp是内核栈栈顶。ch3的trap.S相比ch2的trap.S有所不同，不需要像ch2那样在开头`mv sp, a0`了，[指导书](https://learningos.cn/rCore-Camp-Guide-2025S/chapter3/3multiprogramming.html#id4)中有提到这点。
    
ch2的__retore被调用时会把内核栈顶作为参数通过a0传递给__restore，所以需要在开头`mv sp, a0`把a0(内核栈的栈顶)赋值给sp。
    
而ch3的run_first_task()和run_next_task()直接调用的是__switch，而__switch末尾会`ld sp, 8(a1)`让sp记录内核栈栈顶。所以__restore开始时，sp已经指向内核栈栈顶。
    
那么ch3谁调用了__retore?
    
观察os/src/task/context.rs TaskContext这个结构体，其中有个属性是`ra: __restore as usize`，而在 switch.S 中，有`sd ra, 0(a0)`，这里`0(a0)`为`task_context对象的ra`，`__switch`末尾调用ret，而ret会跳到ra所指向的地址执行，这样__restore就被调用了。

__restore的两种使用情景：
        
1. 开始运行第一个用户程序时需要调用__restore

2. 当trap处理完毕，需要返回继续运行用户程序时，也需要调用__restore

#### 2. L43-L48：这几行汇编代码特殊处理了哪些寄存器？这些寄存器的的值对于进入用户态有何意义？请分别解释。
```asm
ld t0, 32*8(sp)
ld t1, 33*8(sp)
ld t2, 2*8(sp)
csrw sstatus, t0
csrw sepc, t1
csrw sscratch, t2
```
这是在从栈中恢复sstatus, sepc, sscratch这3个CSR寄存器的值。在__alltraps中有这个过程的逆代码。以恢复sstatus为例：__alltraps中有`csrr t0, sstatus; sd t0, 32*8(sp)`两行，这两行代码把sstatus存在了`32*8(sp)`位置，之所以要先用t0暂存是因为sd不能直接操作CSR寄存器。所以，__restore恢复sstatus时从`32*8(sp)`处取值，其它两个寄存器同理。

sstatus和sepc寄存器的意义见[指导书](https://learningos.cn/rCore-Camp-Guide-2025S/chapter2/4trap-handling.html#id9)：
```
sstatus: SPP 等字段给出 Trap 发生之前 CPU 处在哪个特权级（S/U）等信息
sepc: 当 Trap 是一个异常的时候，记录 Trap 发生之前执行的最后一条指令的地址
```
而sscratch是RISC-V架构中的一个S-mode特权级别的寄存器，这里被用来暂存系统/用户栈顶的位置。

#### 3. L50-L56：为何跳过了 x2 和 x4？
    
x2是sp，后面有单独的汇编代码维护sp；x4是线程寄存器tp，ch3实验没有使用到。

#### 4. L60：该指令之后，sp 和 sscratch 中的值分别有什么意义？

这行代码的效果是交换sp和sscratch的值，交换之后sp指向用户栈，而sscratch指向内核栈。也就是从内核栈换到了用户栈。

#### 5. __restore：中发生状态切换在哪一条指令？为何该指令执行之后会进入用户态？
    
最后的sret指令发生状态切换，从S态切换到U态。

关于sret指令的机制，[指导书](https://learningos.cn/rCore-Camp-Guide-2025S/chapter2/4trap-handling.html#trap-hw-mechanism)中有写：

当CPU完成Trap处理准备返回的时候，需要通过一条S特权级的特权指令sret来完成，这一条指令具体完成以下功能：

* CPU会将当前的特权级按照sstatus的SPP字段设置为U或者S；

* CPU会跳转到sepc寄存器指向的那条指令，然后继续执行。

当CPU执行完一条指令并准备从用户特权级陷入（ Trap ）到S特权级的时候，硬件会自动完成：sstatus的SPP字段会被修改为 CPU当前的特权级（U/S），我们是从用户态进入trap的，所以最后sret也就会返回用户态。而在第一次进入用户程序时，os/src/trap/context.rs中有`sstatus.set_spp(SPP::User);`一行，所以也是进用户态。

#### 6. L13：该指令之后，sp 和 sscratch 中的值分别有什么意义？

这行代码的效果同样是交换sp和sscratch的值，执行之后sp指向内核栈顶，sscratch指向用户栈顶，即实现用户栈 -> 内核栈。

#### 7. 从 U 态进入 S 态是哪一条指令发生的？

ecall，通过ecall调用sbi，或者调用会引发异常的指令（例如ch2b_bad_*.rs的例子）时，会从U态进入S态。

## 荣誉准则
1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与以下各位就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：

    rcore-camp群友

2. 此外，我也参考了以下资料 ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：

    问chatgpt和deepseek相关内容

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。 我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。