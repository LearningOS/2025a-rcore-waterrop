1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与 以下各位 就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：

2. 此外，我也参考了 以下资料 ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：
    https://learningos.cn/rCore-Tutorial-Guide-2025S/
    https://course.rs/about-book.html

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。 我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。

1. mutex_lock和semaphore_down如何知道是否应该进行死锁检测？需要有一个变量记录是否启动死锁检测
2. 这个变量是保存在全局变量中？还是在进程的PCB中？
3. 死锁检测

 1) Available,Allocation,Need这些资源保存在哪里？
    Available：表示可利用资源向量，即一个进程里的所有可利用资源的数量，也就是sys_mutex_create创建了多少个锁和sys_semaphore_create创建了多少个信号量。所以应该保存在PCB中，并在create时增加资源。
    Allocation：表示每类资源已分配给每个线程的资源数，是一个n*m的矩阵，n为线程数，每当新创建一个线程，Allocation就要加一行，每当获取一个锁或信号量，Allocation里相应的资源就要增加。保存在PCB中。
    Need：表示每个线程还需要的各类资源数量，是一个n*m的矩阵，need初始等于该线程需要的资源，同Allocation类似，区别在于当获取锁或信号量时，相应资源减少。保存在PCB中。

 2) 这些数据结构用什么存储？

 3) 如何知道线程需要多少个资源？利用request，need=request-allocation，每当线程请求一个资源，request相应加1，并更新need；当真的分配了资源后，allocation更新，同时更新need。request也在PCB中

 4) mutex与semaphore分开记录

 5) mutex_available: Vec<isize>，记录可用资源数
    (1) 增加资源的情况：当sys_mutex_create时，往mutex_available里添加新元素或者重置mutex_id处的值
    (2) 减少资源的情况：因为资源创建后不会删除，知道PCB销毁，所以不用关心这种情况
    (3) 值增加的情况：当sys_mutex_unlock时，某一类资源的可用数增加
    (4) 值减少的情况：当当sys_mutex_lock时，执行了mutex.lock();后，某一类资源的可用数减少

 6) mutex_allocation：Vec<Vec<isize>>，一个n*m的矩阵，mutex_allocation[i][j]=1表示给线程i分配了1个j类资源。
    (1) 增加行数的情况：当sys_thread_create时，为mutex_allocation添加一个新的行，即Vec<isize>（因为线程只有就绪态、运行态和等待态，所以线程只有当PCB销毁时，才会销毁，所以线程在tasks里的索引和在mutex_allocation里的索引一样）。
    (2) 增加列数的情况：当sys_mutex_create时，为mutex_allocation每一行push一个新元素表示新创建的资源。
    (3) 由于资源、线程不会销毁，所以行数、列数不会减少。
    (4) 增加值的时候：当sys_mutex_lock时，执行了mutex.lock();后，表示为线程分配了一个资源，此时值要增加。
    (5) 减少值的情况：当sys_mutex_unlock时，某线程释放了资源，此时值要减少。

 7) mutex_need：Vec<Vec<isize>>，一个n*m的矩阵，mutex_need[i][j]=1表示线程i需要1个j类资源。
    (1) 行数增加的情况：当sys_thread_create时，为mutex_need添加一个新的行，即Vec<isize>
    (2) 列数增加的情况：当sys_mutex_create时，为mutex_need每一行push一个新元素表示新创建的资源。
    (3) 由于资源、线程不会销毁，所以行数、列数不会减少。
    (4) 值增加的情况：当sys_mutex_lock时，表示线程请求一个资源，即需要一个资源，此时值要增加。
    (5) 值减少的情况：当sys_mutex_lock时，执行了mutex.lock();后，某线程获得了资源，即需要的资源减少了，此时值要减少。

 8) sys_mutex_lock流程
    (1) 对应的资源请求加1
    (2) 若开启了死锁检测，则进行死锁检测
    (3) 可以分配：
         allocation+1，need-1
    (4) 不能分配：返回-0xDEAD
 9) sem的情况与mutex一样


4. 问答
 1) TCB、锁、信号量、内存、文件等资源
 2) 可能在内核栈、共享内存上被引用，需要解引用，然后按顺序清除
 3) Mutex2没有loop循环，则可能没有获取锁却进入了临界区