# 功能矩阵与后续验收 · checkpoint 9C JVM diagnostics / 0.8.12

> Public-source note: named raw logs, screenshots and measurement payloads are omitted from this repository. See [verification evidence](../PUBLICATION.md#verification-evidence).

“已实现”指当前代码有可执行路径，不等于所有平台、真实SSH或生产项目均已验收。协议仍为精确版本4，能力声明不授予执行信任。0.8.16已通过同提交Ubuntu/Windows CI，包含真实Java三会话、真实编辑器事务，以及独立任务并发和强制所属agent清理。0.9.0新增正常Windows隔离agent的专用Java/JDT路径，精确提交正式路径CI待验证；通用Windows LSP仍不支持。既往间歇性修正诊断失败的原因尚未证实，不由后续成功运行推断通用可靠性。

| 领域 | 当前实现 | 尚缺 / 下一阶段验收 |
|---|---|---|
| 原生前端 | egui/Glow、主题、窗口、快捷键；Linux 历史窗口验收；第五阶段信任关闭下最终 release 配置检查 | 第六阶段信任关闭能力提示/文件保存已复验；可访问性、完整 IME/无障碍、高 DPI 与 Windows/macOS 运行验收 |
| 编辑器 | UTF-8、多标签、保存、基本高亮、查找、行号、按需系统 CJK 字体回退、预览后显式格式化；磁盘双栏对比与干净标签二次读取重载，保留原生撤销 | rope/增量语法树、超大文件、块选择、代码折叠、差量/持久撤销 |
| 文件与工作区 | 单根目录、相对路径、按需列表、新建文件；晚期 SHA 冲突复核；普通文件 Windows 替换修复及已通过的 Windows CI 回归 | 文件移动/删除、监听、多根、最近项目、完整会话持久化；非原子 compare-and-swap |
| 防丢稿 | 版本冲突拒写、关闭确认、断线保留；本地防抖恢复、精确版本确认、启动审阅/显式恢复、保留原始 base revision | 防抖后未确认输入仍可丢失；非项目自动保存；无断电保证；三方合并、自动外部变动提示、真实多平台恢复验收；脏稿不允许重载 |
| 恢复隐私 | 前端本地明文、Unix 私有权限、单写者锁、完整性校验、有界配额、错误可见；不恢复执行信任 | 无加密/秘密保险箱；Windows 原生恢复和 ACL 隐私验收未完成；无恶意同账户写入者隔离；无静默淘汰旧稿 |
| 远程 | 系统 OpenSSH、stdio agent、统一后端、显式重连；有界传输故障测试；后台直接子进程回收；SSH 配置硬化；唯一有效握手与有界后端能力发现 | **真实 SSH 认证/断网互操作仍是核心门槛**；自动部署/升级、跨协议版本协商、断线事务恢复、旧任务接管、端口管理；旧 OpenSSH 客户端运行验收 |
| Java/Kotlin | 文本编辑、语法着色、JDT LS 诊断/补全/跳转；第二阶段真实 Java 与旧版社区 Kotlin 语义链验证 | 完整 Maven/Gradle 工程模型、SDK 管理、当前官方 Kotlin 许可/兼容性验证、生产项目回归 |
| LSP / 重构 | 自动同步、诊断、悬浮/定义、补全与延迟 import；只读格式化预览、显式 Apply/Cancel、单文档撤销；同步匹配草稿后查引用；显式层级/平面大纲；有界队列/超时 | 引用是无版本结果，不保证目标时效；多文件重命名需先解决快照/资源操作/跨文档撤销安全；inline diagnostics、代码操作、snippets、服务多路化 |
| 调试 | 独立异步 DAP 传输；第二阶段真实 Python 断点/栈/变量/继续/停止验证 | IDE 调试 UI/远程桥接、可靠后代进程回收、监听安全、Java/JVM 调试 |
| 命令运行 | 明确 executable + 字面量 argv；RunStart/Poll/Cancel、实时有界输出、终态区分、运行中继续编辑保存、关闭/重连保护 | 本阶段原生显式 Run/Cancel/重连复核待批准启用信任；Windows 0.7.1 隔离 agent 实际 CI 已通过，原生 GUI 待验收；交互式 PTY、测试结果树、并行任务；macOS 运行未验收 |
| 保存的命令配置 | 显式 Load/选择/新建/Save；`cedar.tasks.json` 严格有界格式；字面量参数行与预览；普通编辑器版本检查、单次撤销和恢复；重连显式复核 | 不是完整运行/调试配置系统；无自动发现、预设、环境变量、目录覆盖、变量展开或 autorun；未序列化表单只在当前会话 |
| Windows进程基础 | 原子Job绑定、私有本机管道、完成后回收的异步I/O、严格argv、固定64KiB owned stdin；0.8.3的48单元+21生命周期+12语言传输全部实际通过 | 0.8.12直接真实Java三会话与3项agent语言夹具已通过；0.8.15真实agent/编辑器三会话已通过；0.8.16真实任务并发/强制所有者清理已通过；不等同GUI或恶意代码沙箱 |
| Windows本地后端 | 同目录精确cedar-agent.exe；固定IsolatedAgent模式、信任独立、异步任务三项能力；绝对原生.exe路径；完整u32退出码 | 无PATH/PATHEXT或batch解析；缺失bundle无回退；Git/同步Run/通用LSP/DAP仍禁用；新增专用Java/JDT路径待0.9.0正式路径CI；原生 Windows GUI 待验收；LSP Job底层已验证，直接真实Java和非分发agent并发夹具已通过；0.8.15真实agent/编辑器已通过；0.8.16任务并发已通过；0.9.0正式Java路径待CI |
| 任务安全 | 配置操作零自动执行；每连接一个异步任务、1–300 秒、每流 256 KiB、8 个历史记录；Linux 普通进程组清理；不自动重试不明结果 | 非 OS 沙箱，恶意逃逸后代可能存活；Unix 强杀 agent 不保证任务清理；Windows 已验证 Job 随所有者退出清理；遗留同步 Run/Git/LSP/DAP 不计入异步任务限额 |
| 搜索 | 有界文本搜索、结果跳转；语言服务引用查找与工作区边界导航 | 正则、替换、全工作区符号索引 |
| Git | 可信工作区状态 | diff/hunks/stage/commit/blame/merge、分支/远端管理 |
| 插件 | Rust crate 扩展边界 | 稳定插件 ABI/协议、权限、生命周期、市场；无 IDEA 插件兼容承诺 |
| 企业功能 | 无 | 数据库、Spring、Web、容器、应用服务器、Profiler、协作等需分别设计 |
| 性能 | 懒加载、按需重绘、有界读取/输出/传输/恢复存储 | 无本阶段新内存基准；历史短时前端读数与 JVM 分开；同项目可复现基线、远程延迟、长会话泄漏、生产项目回归、完整进程树核算 |
| 分发 | Cargo工程、锁文件、许可清单、公开分阶段源码；0.8.3 双平台 CI 全绿，Windows21进程+12语言传输+7agent实际执行；0.8.12直接真实Java三会话与3项agent语言夹具已通过；9D准备见当前测试报告 | 本阶段发布/精确提交 CI；Windows/macOS 原生 GUI、已签名安装包、自动升级与安全发布 |

## 验收边界

- 第五阶段公开提交 [`119d5c30ddba51cbfa9f04d5ae6e281103a3803f`](https://github.com/LLLLimbo/cedar-ide/commit/119d5c30ddba51cbfa9f04d5ae6e281103a3803f) 的 [Ubuntu/Windows CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37643452204) 均通过，包括 Windows 保存、传输故障与独立 agent 回归；不是 Windows GUI 验收
- 环境更换后，第五阶段经 Git tree SHA 精确恢复，重新通过 413 个普通 + 6 个显式 Rust 测试，以及四条 Python agent 链、fmt 与严格 Clippy，见[恢复记录](ENVIRONMENT_RECOVERY.md)
- 0.7.1 的 [Ubuntu/Windows CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37673873489) 均通过；Windows 13 项进程生命周期与 7 项隔离 agent/bundle 全部实际运行，含强杀精确后代清理与同目录 agent 选择
- 第六阶段增加有界元数据、旧/新 reader 兼容、无能力请求零发送、单次握手缓存、生命周期前提、按后端支持门控和脏稿保护；最终本地聚合459项Rust、五条Python agent链和两项导出回归通过，随后同提交Ubuntu/Windows CI通过
- 真实 SSH 认证/断网互操作仍待专门授权；未生成测试密钥、未启动 sshd 或连接用户服务器。配置解析和 stdio 故障测试不替代这道门槛
- 原生信任开启、Run/Cancel/重连复核与脏窗口关闭测试仍未完成；能力元数据不会替代或绕过这些权限
- 历史原生/JDT/Kotlin/debugpy 结果、资源读数和限制保留在各阶段报告；第六阶段不能继承旧二进制的哈希或新平台验收结论

## 建议下一顺序

1. 0.8.16有限原生验收已通过；验证0.9.0正常agent/Client专用Java路径及诚实Stop结果。通用Windows LSP仍禁用，专用Java路径待精确提交CI。Linux历史超时若重现，按具体条件诊断。
2. [Windows Java LSP](WINDOWS_LANGUAGE_PLAN.md)：可取消 stdin 与独占 Job/可加入线程/严格帧解析已实现但仍受门控；通过真实 Windows 语言服务及与任务并发清理测试后才开放能力。初始顺序请求仍有启动阻塞，须明确披露或另做异步生命周期协议
3. 远程核心路径：获得狭窄测试批准后验证真实 SSH 认证、严格主机密钥、远程路径引用、断开/重连和不明结果不重放；明确版本/能力边界、实际进程清理和 Windows 前端到 Linux 后端的证据。此门槛与语言服务工作并行推进准备，不以 stdio 测试替代
4. DAP 调试 UI/agent 与真实程序闭环；先解决监听安全和普通后代进程回收，再扩展 PTY、测试树与 JVM 调试
5. Java/Kotlin 工程导入：真实大型 Maven/Gradle 项目、JDK 选择与索引进度；解决当前官方 Kotlin 许可/安装阻碍后再验兼容性
6. 安全重构：保留严格 URI、UTF-16、版本与脏稿边界；多文件重命名须先解决无版本跨文件快照、跨文档撤销和遗漏文件资源操作，见[重构路线](REFACTORING_ROADMAP.md)
7. Git/插件协议与框架工具，按实际需求逐项实现；在等价功能集下建立可复现性能基线

上述路线不是已经实现的功能或交付承诺。完成后仍不能直接宣称与 IntelliJ IDEA 全功能等价，需要持续维护兼容矩阵、真实测量与长期项目使用反馈。
