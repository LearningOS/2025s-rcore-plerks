# rCore-Camp-2025s ch6报告

## 总结功能
实现三个系统调用 sys_linkat、sys_unlinkat、sys_fstat

思路：

1. sys_linkat

    在DiskInode中增加一个字段`nlink: u32`，表示引用计数。
    
    通过Inode的modify_disk_inode()能从ROOT_INODE这个Inode转到其对应的DiskInode，modify_disk_inode()被设计为传入一个闭包，闭包参数会被给成Inode对应的DiskInode，从而可以让我们读写DiskInode。
    
    为了创建硬链接，首先找到目标文件的inode_id(使用find_inode_id())，然后通过modify_disk_inode()为ROOT_INODE增加DirEntry，name为我们想要创建的链接名，inode_id为目标文件的inode_id，这样就创建了硬链接。然后通过目标文件的Inode，使用modify_disk_inode()增加nlink即可。

2. sys_unlinkat

    首先要删除ROOT_INODE中对应的DirEntry。然后要找到对应的target_inode，减小其引用计数，如果引用计数为0，要彻底删除文件，回收inode以及它对应的数据块。

    回收inode通过EasyFileSystem中的inode_bitmap，将对应位置0；回收数据块则使用Inode.clear()

    过程中需要注意避免fs.lock()的重复调用，以及modify_disk_inode()和read_disk_inode()嵌套起来调用的问题，否则会因为重入fs和BLOCK_CACHE_MANAGER导致死锁。

3. sys_fstat

    在Inode中增加get_inode_id、get_mode、get_nlink等函数，从fd_table中我们能拿到OSInode，OSInode中能拿到Inode，从而可以调用增加的函数获取文件信息。

## 简答作业

### 在我们的easy-fs中，root inode起着什么作用？如果root inode中的内容损坏了，会发生什么？
在os/src/fs/inode.rs中，通过lazy_static!，我们打开了EasyFileSystem，并初始化了变量ROOT_INODE，这是文件系统根目录`/`的Inode。通过它我们才能寻找、增加文件等，如果root inode中的内容损坏了，文件系统的加载就会有问题。

# rCore-Camp-2025s ch7报告
ch7有为os增加管道、命令行参数与标准I/O重定向的功能，但是没有要完成的编程作业，指导书要求ch7的问答作业写在ch6的报告里，一并提交，不需要单独为ch7写报告。

## 简答作业

### 1.举出使用 pipe 的一个实际应用的例子。
tips:

* 想想你平时咋使用 linux terminal 的？

* 如何使用 cat 和 wc 完成一个文件的行数统计？

例如`cat .bashrc | wc -l`，shell会将`cat`的标准输出，通过管道传递给`wc`(将管道的读fd通过dup，变成stdin)，这样`wc`就能直接从stdin读自己要统计行数的输入。

关于shell将`wc`的标准输入偷换成管道的fd的过程，[rcore-camp-guide](https://learningos.cn/rCore-Camp-Guide-2025S/chapter7/2cmdargs-and-redirection.html#id3)有说，在user/src/bin/ch7b_user_shell.rs中：

```Rust
if pid == 0 {
    ...
    close(0);
    assert_eq!(dup(input_fd), 0);
    close(input_fd);
    ...
    exec(...)
}
```

先关闭标准输入fd 0，dup管道的input_fd，由于fd分配的方式，dup一定会分配我们刚刚关闭的fd 0，然后再关闭input_fd，再exec。这样`wc`子进程的标准输入就会是管道的read fd了，其能无感地从管道中读取自己要统计的内容。

### 2. 如果需要在多个进程间互相通信，则需要为每一对进程建立一个管道，非常繁琐，请设计一个更易用的多进程通信机制。

[rcore-tutorial](https://rcore-os.cn/rCore-Tutorial-Book-v3/chapter7/5exercise.html#id7)的编程作业有个设计：

这一章我们实现了基于 pipe 的进程间通信，但是看测例就知道了，管道不太自由，我们来实现一套乍一看更靠谱的通信 syscall吧！本节要求实现邮箱机制，以及对应的 syscall。

邮箱说明：每个进程拥有唯一一个邮箱，基于“数据报”收发字节信息，利用环形buffer存储，读写顺序为 FIFO，不记录来源进程。每次读写单位必须为一个报文，如果用于接收的缓冲区长度不够，舍弃超出的部分（截断报文）。为了简单，邮箱中最多拥有16条报文，每条报文最大长度256字节。当邮箱满时，发送邮件（也就是写邮箱）会失败。不考虑读写邮箱的权限，也就是所有进程都能够随意给其他进程的邮箱发报。

## 荣誉准则
1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与以下各位就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：

    rcore-camp群友

2. 此外，我也参考了以下资料 ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：

    问chatgpt和deepseek相关内容

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。 我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。