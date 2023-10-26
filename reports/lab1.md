# 简单总结
本实验主要查看了源代码中task模块的实现，因为考虑taskinfo中的status，调用统计和时长都是和task有关，因此将process.rs原有的taskinfo结构体直接移动到了task，这样task的TaskControlBlock结构体统一维护，也便于在process中方便调用
# 简答作业
### 1. 正确进入 U 态后，程序的特征还应有：使用 S 态特权指令，访问 S 态寄存器后会报错。 请同学们可以自行测试这些内容 (运行 Rust 三个 bad 测例 (ch2b_bad_*.rs) ， 注意在编译时至少需要指定 LOG=ERROR 才能观察到内核的报错信息) ， 描述程序出错行为，同时注意注明你使用的 sbi 及其版本
```
[rustsbi] RustSBI version 0.3.0-alpha.2, adapting to RISC-V SBI v1.0.0
...
[kernel] PageFault in application, bad addr = 0x0, bad instruction = 0x804003c4, kernel killed it.
[kernel] IllegalInstruction in application, kernel killed it.
[kernel] IllegalInstruction in application, kernel killed it.
```
由于U模式下触发了trap_handler中S模式下的异常，因此经过异常处理打印了以上异常信息
## 2. 深入理解 trap.S 中两个函数 __alltraps 和 __restore 的作用，并回答如下问题:
1) L40：刚进入 __restore 时，a0 代表了 要恢复的上下文的sp地址，异常和系统调用上下文切换恢复的时候会用到__restore
2） 
ld t0, 32*8(sp)  
ld t1, 33*8(sp)
ld t2, 2*8(sp)
csrw sstatus, t0 恢复S态的状态寄存器
csrw sepc, t1 恢复用户PC指针
csrw sscratch, t2 恢复上下文指针
3） 因为x2寄存器即为sp，x4位线程指针也就是TP
4）L60：该指令之后，sp 和 sscratch 中 前者代表用户态上下文指针，sscratch代表内核上下文指针
5)sret 会从内核态切回到用户态
6) 与上面4）相反，SP为内核态，sscratch为用户态上下文
7） 从 U 态进入 S 态是哪一条指令发生的？ ecall


# 荣誉准则
在完成本次实验的过程（含此前学习的过程）中，我曾分别与 以下各位 就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：

**主要参考rCore-Tutorial-Guide-2023A**

此外，我也参考了 以下资料 ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：

**主要参考rCore-Tutorial-Guide-2023A和代码**

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。 我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。
