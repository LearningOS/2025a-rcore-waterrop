在完成本次实验的过程（含此前学习的过程）中，我曾分别与 以下各位 就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：



此外，我也参考了 以下资料 ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：

https://rcore-os.cn/rCore-Tutorial-Book-v3/chapter4/3sv39-implementation-1.html#id5
https://learningos.cn/rCore-Tutorial-Guide-2025S/chapter4/0intro.html
AI
https://course.rs/about-book.html

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。 我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。



sys_spawn
一个PCB就是一个进程，想要创建一个进程的话，新创建一个PCB即可。
再把这个新建的PCB加入父进程，并进行父子进程的连接建立。
最后加入调度器
父进程是当前使用CPU的进程。
因为要执行目标程序，所以目标程序的执行与exec类似。

stride 调度算法
1) 需要添加stride，pass，priority，BigStride
   其中stride、pass、priority在运行过程中是变化的，添加到TaskControlBlockInner中；BigStride是不变的，添加到TaskManager中。
2) 子进程是否继承父进程的优先级？应该一致
3) 修改TaskManager里的pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>>
   使用暴力遍历找到min_stride
   修改stride=stride+pass
4) 创建进程时，设立初始优先级

问答作业
1) 轮不到p1执行，因为stride只有8位，最大表示255，而p2执行完stride+pass为260，溢出了，会导致p2实际上比p1的255小，所以仍会执行p2
2) prio>=2导致pass<=BIG_STRIDE/2，当选择了最小的stride时，需要加上pass，使其在执行完的时候的stride尽可能达到最大，所以TRIDE_MAX – STRIDE_MIN <= BigStride / 2
3) fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let s_val = self.0;
        let other_val = other.0;
        let half = BIG_STRIDE / 2;
        // 判断 s 是否比 other 小（处理溢出）
        let s_less_than_other = if s_val < other_val {
            // 正常情况：s_val 小，且差距未超 half
            (other_val - s_val) <= half
        } else if s_val > other_val {
            // 溢出情况：s_val 看似大，但差距超 half, 实际更小
            (s_val - other_val) > half
        } else {
            // 永远不相等，直接返回 false
            false
        };
        // 适配最大堆：s 更小时，让它被视为“更大”，优先弹出
        if s_less_than_other {
            Some(Ordering::Greater)
        } else {
            Some(Ordering::Less)
        }
    }



