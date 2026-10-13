# 功能矩阵与后续验收 · 0.63.2 浏览外壳候选（待验收）

> Public-source note: named raw logs, screenshots and measurement payloads are omitted from this repository. See [verification evidence](../PUBLICATION.md#verification-evidence).

当前 **0.63.2 浏览外壳候选尚未验收**，在已限定验收的 0.62.2 基线上整理连接后的 Project、编辑器、Find、Workspace / Tools 菜单及 Back/Forward 位置。Alt+1 保留 Find 并聚焦有用的 Project 控件，Ctrl/Cmd+J 重开 Find 时选择当前可见有效行，否则聚焦查询框，隐藏时返回捕获的编辑器上下文。上述变更仍须单独验证；具体范围和几何目标见[候选报告](TEST_REPORT_PHASE63_BROWSING_SHELL.md)。后端、协议、新搜索选项及 Build/Run 行为不在本次范围，不声明 IDEA 等价或性能优势。

最新已限定验收基线为 **0.62.2**，公开提交 [`91aa84cb79547207ed4deb296a61834df26089ef`](https://github.com/LLLLimbo/cedar-ide/commit/91aa84cb79547207ed4deb296a61834df26089ef) 的 [CI 38282992861](https://github.com/LLLLimbo/cedar-ide/actions/runs/38282992861) 四项作业与三种精确源码包通过；Linux 2,031/0/70，Windows 1,969/0/114（通过/失败/忽略）。58 项 Browse、43 项 Search、13 项范围搜索帧检查及精确 7/12/10 操作普通 agent 证明通过。新的精确包 Linux 原生检查覆盖两个 Find 视图的单击选择、显式 Preview、真实系统双击 Open、`src` / `*.rs` 范围、分组折叠/展开、脏稿 Back/Undo/Redo 和退出码 0；可选无效范围、空掩码、较宽 OR 掩码、编辑后停用、模态重开与窄几何未运行。确定性保护与新原生观察分开计入，Windows GUI、真实 SSH 仍未验收；历史失败及原因未知的旧原生意外 Open 均保留在[当前报告](TEST_REPORT.md)。

“已实现”指有可执行路径，不等于所有平台、真实SSH或生产项目均已验收。协议仍为版本4，能力声明不授予执行信任。0.9.0专用Windows Java正常agent/Client路径已通过同提交双平台CI，并如实报告强制Stop；通用Windows LSP仍不支持。0.10.0会话内显式断线保存核对已通过精确提交双平台CI，九项真实进程故障用例在两端均执行通过。

| 领域 | 当前实现 | 尚缺 / 下一阶段验收 |
|---|---|---|
| 浏览外壳（0.63.2 待验收候选） | 连接后的 Project / 编辑器 / Find 为主要布局；Workspace 收纳设置，保留连接、错误、恢复及脏稿状态；Tools 保留 Git / Commands / Language / Tests；标签带 Back/Forward；Alt+1 与 Ctrl/Cmd+J 的候选焦点路径 | 新布局、菜单可达性、失效/隐藏命中、编辑器返回及 780 × 540 / 13 与 18 字号仍须验证；40/48 逻辑像素为连接头部设计目标；28/34 为工具标题行最小高度，外层边距、分隔线与正文另占高度；不是 IDEA 像素或固定高度保证；既有基线验收不能替代这些检查 |
| 保留分组文本结果（0.62.2 已限定验收基线） | 共用一份同会话 500 行/512 KiB 历史快照；精确路径分组和原始命中身份；单击选择、Space 预览、同一视图同一行的真实双击或 Enter 普通 Open；隐藏/重开及 Open–Back 保留查询、选择、展开和各视图滚动；窗口与底部 FIND RESULTS 共用结果 | 58 项 Browse 与 43 项 Search 帧检查及保留普通 agent 证明通过；新 Linux 原生覆盖单击/Preview/双击，未重跑可选编辑后停用、模态重开与窄几何；折叠或完全不可见的行不可激活；过短显式工具高度可用 Automatic height 恢复；无防抖或跨重启持久化 |
| 范围文本搜索（0.62.2 已限定验收基线） | 共用 Directory 和分号分隔的完整文件名 `*` / `?` 掩码；后端从范围目录遍历、打开内容前匹配掩码；协商 scoped_search v1，无支持时零新请求及零隐式扩大；空筛选沿用旧 Search，修改后须显式成功重跑 | 13 项范围帧检查及 52 字段/12 用例/10 操作普通 agent 证明通过；新 Linux 原生覆盖 `src` / `*.rs`，可选无效范围、空掩码和较宽 OR 掩码未运行；有独立预算、排除项及不完整状态；无大小写/整词开关、正则、CamelCase 或整个 B2 完成声明 |
| Find and return（0.62.2 已限定验收基线） | Find…、Ctrl+Shift+N / Ctrl+Shift+F 进入 Files / Project Text；显式提交、选择、只读预览、普通 Open 与已有 Back；打开草稿预览零 Read、保留脏稿与 Undo，无隐式 Save；协商支持的 Project files 扫描未展开目录，不支持时明确回退已打开/已加载快照 | 完整双平台 CI、三种包与限定 Linux 原生验收通过；0.61 的原生 Ctrl+Alt+Left/Right 无导航限制原因仍未知；预览限 1 MiB，文本行是历史提示；Back 仅恢复仍打开且编辑版本未变的位置；Double Shift 与共享 Java Types 入口尚未实现 |
| 有界文件名发现（0.60.0 已限定验收基线） | 可选 Hello 扩展版本 1；协议 4、AgentInfo schema 1、直接能力与分组上限保持；根目录或显式相对目录范围，100 个界面结果、20,000 条目、深度 64、协作式两秒及路径/响应/待遍历目录预算；明确排除目录与不完整原因 | 不是完整索引或 `.gitignore` 解释器；不完整扫描无匹配不证明文件不存在，排序只覆盖观察到的候选；不跟随符号链接/reparse point，无新增执行信任、传输或恶意并发命名空间沙箱声明 |
| 原生前端 | egui/Glow、主题、窗口、快捷键；0.56.1 已验收实际鼠标主键竖向拖动后的进程内显式工具高度，未拖动仍自动收缩、重启恢复自动；0.58.0 增加的工具栏 Automatic height 显式清除手动高度，布局结束后重绘恢复既有自动收缩，随后真实拖动可重新设置 | 0.58.4 完整双平台 CI、三种包与限定 Linux 原生真实拖动/鼠标及键盘重置/Tab→立即 Undo/Redo 已通过；15 项 Tab、27 项高度及 23 项侧栏检查通过，历史夹具失败保留；小窗口限制不覆盖偏好，放大可恢复，无内容硬上限/编辑区空间保证；可访问性、完整 IME/无障碍、高 DPI 与 Windows/macOS GUI 验收 |
| 编辑器 | UTF-8、多标签、保存、已验收的选区 Tab/Shift+Tab 缩进、基本高亮、查找、单文档字面量替换预览与单次撤销、行号、键盘文件选择器与跳到行、已验收的 Unicode 标量行:列导航；已验收的选区内字面量替换预览；Ctrl/Cmd+D 整行复制（0.54.0 已验收）、按需系统 CJK 字体回退、预览后显式格式化；磁盘双栏对比与干净标签二次读取重载；严格分离改动的显式脏稿合并预览/二次读取/单次撤销 | 0.31.1已打开标签位置历史已通过双平台CI与Linux原生检查；rope/增量语法树、超大文件、块选择、代码折叠、差量/持久撤销 |
| 文件与工作区 | 单根目录、相对路径、按需列表、0.30显式展开的有界目录树（双平台普通agent与Linux原生GUI已验收）、新建文件；晚期 SHA 冲突复核；普通文件 Windows 替换修复及已通过的 Windows CI 回归 | 文件移动/删除、监听、多根、最近项目、完整会话持久化；非原子 compare-and-swap |
| 防丢稿 | 版本冲突拒写、关闭确认、断线保留；0.32普通保存回执的提交内容哈希及身份核对已通过精确提交双平台CI；显式双读取核对未知保存结果，保留后续草稿与撤销；本地防抖恢复、精确版本确认、启动审阅/显式恢复、保留原始 base revision | 防抖后未确认输入仍可丢失；非项目自动保存；无断电保证；通用三方合并、自动外部变动提示、真实多平台恢复验收；脏稿不允许直接重载；合并仅支持严格分离文本区间 |
| 恢复隐私 | 前端本地明文、Unix 私有权限、单写者锁、完整性校验、有界配额、错误可见；不恢复执行信任 | 无加密/秘密保险箱；Windows 原生恢复和 ACL 隐私验收未完成；无恶意同账户写入者隔离；无静默淘汰旧稿 |
| 远程 | 系统 OpenSSH、stdio agent、统一后端、显式重连；空闲时被动感知管道 EOF/错误（无心跳、不保证静默断网检测）；有界传输故障测试；后台直接子进程回收；SSH 配置硬化；唯一有效握手与有界后端能力发现 | **真实 SSH 认证/断网互操作仍是核心门槛**；自动部署/升级、跨协议版本协商、跨应用重启的事务恢复、旧任务接管、端口管理；旧 OpenSSH 客户端运行验收 |
| Java/Kotlin | 文本编辑、语法着色、JDT LS 诊断/补全/跳转；0.22显式Windows离线Maven叶POM/缓存/模型快照；0.29.1直接声明与JDT观察分开的依赖来源快照（精确原生验收通过） | 完整 Maven/Gradle 工程模型、父工程/reactor/profiles与自定义构建、SDK 管理、当前官方 Kotlin 许可/兼容性验证、生产项目回归 |
| LSP / 重构 | 自动同步、诊断、悬浮/定义、补全与延迟 import；显式Java刷新与待到达/过期/无版本提示；当前Java导入整理预览（0.20精确双平台CI已通过）；只读格式化预览、显式 Apply/Cancel、单文档撤销；同步匹配草稿后查引用；显式 Java 实现位置查询及工作区内导航（0.28.1 精确原生验收通过）；显式层级/平面大纲；有界队列/超时；0.36 Linux 所有者进程组/非阻塞管道清理与未验证重启保护（精确双平台 CI 已通过）；0.37 Linux 独立 agent 基本类型化 Java 已通过精确 Ubuntu 原生验收；0.59.2 已限定验收的显式 [Parameter info](JAVA_PARAMETER_INFO.md)：精确同步版本、有界只读签名/参数、无自动触发和参数 0 猜测 | 0.59.3 实际渲染器 Unicode 测试已通过精确 CI 与三种包；原生证据沿用 0.59.2 精确包及已核实生产等价审查，无新 GUI，当前工作区未保有原始私有原生附件；0.16刷新已通过原生验收，Java偶发诊断缺失根因仍未确认；0.17后台启动/取消已通过精确提交双平台验收；引用是无版本结果，不保证目标时效；多文件重命名需先解决快照/资源操作/跨文档撤销安全；inline diagnostics、代码操作、snippets、服务多路化 |
| 调试 | 独立异步 DAP 传输；第二阶段真实 Python 断点/栈/变量/继续/停止验证 | IDE 调试 UI/远程桥接、可靠后代进程回收、监听安全、Java/JVM 调试 |
| 命令运行 | 明确 executable + 字面量 argv；RunStart/Poll/Cancel、实时有界输出、终态区分、运行中继续编辑保存、关闭/重连保护 | 本阶段原生显式 Run/Cancel/重连复核待批准启用信任；Windows 0.7.1 隔离 agent 实际 CI 已通过，原生 GUI 待验收；命令输出横幅仅列入后续待办，未在 0.59.3 实现；交互式 PTY、执行联动的测试树、并行任务；macOS 运行未验收 |
| Java 编译位置 | 已完成任务的显式英文 javac 位置提取、命令/会话绑定、相对路径导航、脏稿保留 | 0.21.1编译器工作流已通过双平台发布验证；历史行号不保证当前草稿位置，绝对路径禁用；无本地化/构建工具格式、执行联动测试树或自动编译 |
| 测试报告 | 显式单文件 Surefire/JUnit XML 阅读、状态/名称筛选与失败详情；历史 revision/连接绑定，信任关闭可用；0.55.4 已验收筛选后的 Up/Down/Home/End、详情与 Back to case；0.57.6 两端各 19 项报告键盘测试和保留进程证明通过 | 0.58.4 通用工具高度重置的 15 用例/55 字段/10 请求/两个所有者回收证明已通过；0.58.0 至 0.58.3 的跳过与失败记录保留；单 testsuite 子集，无自动运行/扫描/刷新、源码推测跳转、任意报告方言或测试发现；不代表 Windows GUI 或任意视口验收 |
| 保存的命令配置 | 显式 Load/选择/新建/Save；`cedar.tasks.json` 严格有界格式；字面量参数行与预览；普通编辑器版本检查、单次撤销和恢复；重连显式复核 | 不是完整运行/调试配置系统；无自动发现、预设、环境变量、目录覆盖、变量展开或 autorun；未序列化表单只在当前会话 |
| Windows进程基础 | 原子Job绑定、私有本机管道、完成后回收的异步I/O、严格argv、固定64KiB owned stdin；0.8.3的48单元+21生命周期+12语言传输全部实际通过 | 0.8.12直接真实Java三会话与3项agent语言夹具已通过；0.8.15真实agent/编辑器三会话已通过；0.8.16真实任务并发/强制所有者清理已通过；不等同GUI或恶意代码沙箱 |
| Windows本地后端 | 同目录精确cedar-agent.exe；固定IsolatedAgent模式、信任独立、异步任务三项能力；绝对原生.exe路径；完整u32退出码 | 无PATH/PATHEXT或batch解析；缺失bundle无回退；旧GitStatus/同步Run/通用LSP/DAP仍禁用；显式Git变更/单文件diff已通过0.15.4同提交原生验收；新增专用Java/JDT正常路径已通过0.9.0 CI；原生 Windows GUI 待验收；LSP Job底层已验证，直接真实Java和非分发agent并发夹具已通过；0.8.15真实agent/编辑器已通过；0.8.16任务并发已通过；0.9.0正式Java路径已通过 |
| 任务安全 | 配置操作零自动执行；每连接一个异步任务、1–300 秒、每流 256 KiB、8 个历史记录；Linux 普通进程组清理；不自动重试不明结果 | 非 OS 沙箱，恶意逃逸后代可能存活；Unix 强杀 agent 不保证任务清理；Windows 已验证 Job 随所有者退出清理；遗留同步 Run/Git/LSP/DAP 不计入异步任务限额 |
| 项目文本搜索（0.57.6 历史基线） | 0.57.6 限定验收通过的显式有界文本搜索、历史提交查询与磁盘行；聚焦行 Up/Down/Home/End 有序移动、边界停止和显示，Enter/Space/点击精确行普通打开；最多 500 行及 512 KiB 路径/片段；语言服务引用查找、显式 Java 类型索引查询 | 双平台 33 项 Search、14 用例/57 字段/12 请求证明、三种包通过；Linux 原生 Space 97→独立点击 96、重新启用与重连可见流程通过；编辑查询后禁用行精确点击命中未证实，保护由 CI/进程证明建立；历史失败原因未知，无原始批次/原生 agent 回收/Windows GUI 声明；查询/连接变化及等待中旧行禁用；无当前草稿匹配、正则、跨文件替换或通用符号索引保证 |
| Git | 可信工作区显式状态与单文件 staged/unstaged diff；明确Git 2.45+路径；根目录/字面量路径/有界进程；旧agent保留状态回退 | 0.15.4新路径已通过双平台真实Git验收；过滤器仍可执行代码；无后台轮询/暂存/提交/重置/抓取；linked worktree、子模块、冲突diff、blame/merge、分支/远端管理尚缺 |
| 插件 | Rust crate 扩展边界 | 稳定插件 ABI/协议、权限、生命周期、市场；无 IDEA 插件兼容承诺 |
| 企业功能 | 无 | 数据库、Spring、Web、容器、应用服务器、Profiler、协作等需分别设计 |
| 性能 | 懒加载、按需重绘、有界读取/输出/传输/恢复存储 | 无本阶段新内存基准；历史短时前端读数与 JVM 分开；同项目可复现基线、远程延迟、长会话泄漏、生产项目回归、完整进程树核算 |
| 分发 | Cargo 工程、锁文件、许可清单、公开分阶段源码；0.35 已通过精确双平台 CI、Windows 开发包及 Ubuntu 24.04 amd64 agent 包的逐文件哈希和真实 stdio 验收 | 0.36 Linux 通用 LSP 清理已通过精确提交双平台 CI/包验收；0.37 Linux 独立 agent 类型化 Java 与双平台包已验收；0.38 能力分组读取兼容性已通过精确双平台 CI；0.39.1 Linux Maven 扩展已通过精确 Ubuntu 验收；未签名，Windows 包需 VC++ x64 运行库；Windows/macOS 原生 GUI、已签名安装包与自动升级仍未验收 |

## 验收边界

- 第五阶段公开提交 [`119d5c30ddba51cbfa9f04d5ae6e281103a3803f`](https://github.com/LLLLimbo/cedar-ide/commit/119d5c30ddba51cbfa9f04d5ae6e281103a3803f) 的 [Ubuntu/Windows CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37643452204) 均通过，包括 Windows 保存、传输故障与独立 agent 回归；不是 Windows GUI 验收
- 环境更换后，第五阶段经 Git tree SHA 精确恢复，重新通过 413 个普通 + 6 个显式 Rust 测试，以及四条 Python agent 链、fmt 与严格 Clippy，见[恢复记录](ENVIRONMENT_RECOVERY.md)
- 0.7.1 的 [Ubuntu/Windows CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37673873489) 均通过；Windows 13 项进程生命周期与 7 项隔离 agent/bundle 全部实际运行，含强杀精确后代清理与同目录 agent 选择
- 第六阶段增加有界元数据、旧/新 reader 兼容、无能力请求零发送、单次握手缓存、生命周期前提、按后端支持门控和脏稿保护；最终本地聚合459项Rust、五条Python agent链和两项导出回归通过，随后同提交Ubuntu/Windows CI通过
- 真实 SSH 认证/断网互操作仍待专门授权；未生成测试密钥、未启动 sshd 或连接用户服务器。配置解析和 stdio 故障测试不替代这道门槛
- 原生信任开启、Run/Cancel/重连复核与脏窗口关闭测试仍未完成；能力元数据不会替代或绕过这些权限
- 历史原生/JDT/Kotlin/debugpy 结果、资源读数和限制保留在各阶段报告；第六阶段不能继承旧二进制的哈希或新平台验收结论

## 历史建议顺序（保留记录）

下列顺序保留先前规划背景，不是 0.63.2 候选的交付顺序。当前在 0.62.2 已限定验收基线上推进连接后的浏览外壳，完成本候选验证后，再按[浏览路线](BROWSING_PRODUCT_ROADMAP.md)推进其余搜索深度与语义导航；既有执行与调试模块不因优先级调整而删除。

1. 0.10.0显式断线保存核对已通过双平台验证；0.11.0慢连接/只读请求取消已通过原生双平台CI；进程树工作集观测已获得，0.11.1 CPU计时窗口修正也已验证。0.12.0固定两次长观察基线已通过原生验收；后段CPU很低但工作集仍高。0.13.0一次固定GC数值诊断已通过原生验证：G1、末次GC后378MiB/容量512MiB，非最终空闲存活量；生产设置不变。0.14.0带哈希清单和中文入门的Windows开发包已通过原生中文路径解压、信任关闭文件操作及双平台CI；0.14.1补充实际VC++ x64运行库前提。内存/空闲CPU基线应包括完整进程树与JVM，无同项目等价条件不声称优于IDEA。
2. [Windows Java LSP](WINDOWS_JAVA_SETUP.md)：专用 Java 正常 agent/Client 路径已开放并通过真实语言服务、编辑事务、任务独立性及强制退出清理验证；仍需显式执行信任。通用 Windows LSP 不支持，原生 GUI 与真实 SSH 验证仍单独待完成。0.17为支持完整可选能力的agent增加后台启动/取消；旧agent仍顺序启动。已启动会话的查询与Stop仍可能阻塞，Stop如实报告强制清理。初始化期间文件操作的新路径已通过0.17精确提交原生验收
3. 远程核心路径：获得狭窄测试批准后验证真实 SSH 认证、严格主机密钥、远程路径引用、断开/重连和不明结果不重放；明确版本/能力边界、实际进程清理和 Windows 前端到 Linux 后端的证据。此门槛与语言服务工作并行推进准备，不以 stdio 测试替代
4. DAP 调试 UI/agent 与真实程序闭环；先解决监听安全和普通后代进程回收，再扩展 PTY、测试树与 JVM 调试
5. Java/Kotlin 工程导入：真实大型 Maven/Gradle 项目、JDK 选择与索引进度；解决当前官方 Kotlin 许可/安装阻碍后再验兼容性
6. 安全重构：保留严格 URI、UTF-16、版本与脏稿边界；多文件重命名须先解决无版本跨文件快照、跨文档撤销和遗漏文件资源操作，见[重构路线](REFACTORING_ROADMAP.md)
7. Git/插件协议与框架工具，按实际需求逐项实现；在等价功能集下建立可复现性能基线

上述路线不是已经实现的功能或交付承诺。完成后仍不能直接宣称与 IntelliJ IDEA 全功能等价，需要持续维护兼容矩阵、真实测量与长期项目使用反馈。

## Linux 基本类型化 Java 边界

0.37 仅向 Linux 独立 agent 开放已有的基本 Java/JDT 路径，能力总数 31、协议上限仍为 32。内嵌 Client Local 保留通用 LSP；0.39.1 使用两个有界能力分组扩展 Linux Maven 三项操作，已通过精确 Ubuntu 原生验收。清理未验证会阻止重启，Linux 信号不伪装成退出码；匹配版本的前端才可解析新的 Stop 回执。此处不表示 SSH、完整 Maven/Gradle、原生 Windows GUI 或资源优化已验收。

0.38 能力分组兼容性已通过精确双平台 CI。0.39 Linux 独立 agent 保留 31 个直接能力名，增加两个精确版本化 Maven 分组；Windows 的直接能力集合不变，协议 4、schema 1、直接能力上限 32 均不变。内嵌 Client Local 仍不支持类型化 Maven；0.41.2 GUI Local 的同目录独立 agent 路径已通过精确双平台 CI、三种包校验及云端 Debian 13 X11 信任关闭 GUI 文件操作检查；未在 GUI 中执行 Java/Maven，未独立观察原生 agent 回收。

0.40 在替换当前连接前复用传输层的纯 SSH 字段校验，并明确端口覆盖与字面量路径语义；此修正已通过 0.40 精确提交双平台验收，不代表真实 SSH 联机已验证。

0.42 增加显式、有界的 [Save All](SAVE_ALL.md)：仅编辑器缓冲区，逐个条件写入，冲突、未知结果或候选变化停止后续提交；不是多文件原子事务，不自动 Run 或序列化配置表单。本阶段验收结果见当前测试报告。

0.44.1 已通过双平台 CI 与限定 Linux 原生窗口验证的恢复存储不可用生命周期保护：编辑及显式 Save/Save All 独立于恢复存储状态，退出时区分未执行、已确认和不明结果，保留剩余副本须明确选择。此阶段不启用 Windows ACL 强制检查，不自动修复权限或切换恢复目录。

0.44.2 仅修订退出原因提示，区分未能检查恢复存储与未确认的已请求删除；保守退出门槛和副本不确定性保持不变，已通过双平台 CI 与限定 Linux 原生退出提示验证。

0.45.0 仅增加非分发的 Windows 恢复存储句柄准入适配器与故障模型，尚不接入生产 Store，不增加原生权限探测。严格所有者与 DACL 候选规则仍可能使恢复不可用；真实允许路径兼容性尚未验证。

0.46.0 拟对新适配器执行一次源版本绑定的 Windows 拒绝路径观察；即使通过，也不代表生产恢复权限检查或真实允许路径已验收。

0.47.4 选中代码块 Tab/Shift+Tab 缩进已保留选区与一次撤销，文本及结果上限 1 MiB、选中逻辑行上限 4,096；双平台 CI、进程证明与三个分发包已验收，原生 GUI 使用精确 0.47.0 包及后续发货代码等价审查，见[最终报告](TEST_REPORT_PHASE47_INDENT.md#final-verified-acceptance--0474)。0.48.1 有界普通 Enter 行首空白延续及折叠 CRLF 光标已验收；0.48.2 的选区上端点收缩策略已验收；0.49.2 的已加载文件选择范围已验收，见[阶段报告](TEST_REPORT_PHASE49_LOADED_FILES.md#final-verified-acceptance--0492)。已完成的 0.46 原生拒绝观察不再由 CI 自动触发，Windows 恢复权限生产接入继续暂停。

0.54.0 键盘整行复制已通过精确提交双平台 CI、22 项新增测试、20 用例/47 字段正常 agent 进程证明及三种分发包验收。新一轮精确包的云端 Linux GUI 七项检查覆盖 LF/CRLF、折叠/正反向选区、EOF、Undo/Redo 和显式丢弃退出；不代表 Windows GUI、原始光标 affinity、请求数或 agent 回收观察，详见[最终报告](TEST_REPORT_PHASE54_DUPLICATE_LINES.md#final-accepted-0540-evidence)。0.55.0 的 Tests 键盘导航首次双平台 CI 通过严格检查，但聚合均有三项新增测试失败；[失败证据](TEST_REPORT_PHASE55_REPORT_KEYBOARD.md#preserved-0550-first-ci-aggregate-failures)完整保留。0.55.1 双平台严格检查与聚合通过，但双平台报告进程驱动均以未定位阶段的通用错误失败，[该轮证据](TEST_REPORT_PHASE551_REPORT_FIXTURES.md#preserved-0551-process-driver-failure)保留。0.55.2 的双平台有界诊断确认进程证明在运行时的同一可见性断言失败，但未识别确切调用处或几何，[证据](TEST_REPORT_PHASE552_REPORT_DIAGNOSTICS.md#preserved-0552-runtime-visibility-failures)保留。0.55.3 的 19 项报告测试、双平台 14 用例/54 字段进程证明及限定 Linux 原生 GUI 均通过，仅有两个 Linux 包通过核验；Windows Java Quick 的版本 5 修正虽被确认，却缺少匹配诊断见证，未产生 Windows 包，[阶段证据](TEST_REPORT_PHASE553_REPORT_GEOMETRY.md#verified-0553-report-scope-and-preserved-windows-java-failure)保留。0.55.4 仅补充该修正观察的测试诊断，不改 JDT 行为、期限、刷新或重试；完整双平台 CI、19 项报告测试、14 用例/54 字段进程证明及三种包已通过，原生证据沿用精确 0.55.3 包及发货代码等价审查，没有新的 0.55.4 GUI 运行。此次 Quick 通过不解释原失败原因；[最终验收](TEST_REPORT_PHASE554_QUICK_DIAGNOSTICS.md#final-accepted-0554-evidence)保留原边界。0.56.0 的新报告加载夹具使首次双平台聚合失败，原始报告和[失败证据](TEST_REPORT_PHASE56_TOOLS_HEIGHT.md#preserved-0560-first-ci-aggregate-failure)保留。0.56.1 仅补齐夹具必需的零值计数和断言，显式高度发货实现不变；已通过精确双平台 CI（Linux 1,747 通过/65 忽略，Windows 1,690 通过/109 忽略）、三种包与限定 Linux 原生组合验收。原始原生运行的窗口目标错误及缩放遗漏保留，另一同包检查补齐实际缩小/放大；不代表硬内容高度上限或编辑区空间保证，见[最终报告](TEST_REPORT_PHASE561_TOOLS_HEIGHT.md#final-accepted-0561-evidence)。0.57.6 的限定 Search 验收及历史失败保留于[原报告](TEST_REPORT_PHASE576_SEARCH_MIXED_INPUT.md#final-accepted-0576-evidence)，其中编辑查询后的禁用行精确点击命中仍未证实。此前完整限定验收 0.58.4 的证据为：公开提交 `9e925cc4dc084b3893bf7ed0cc907990bb22047b`、[CI 38174023016](https://github.com/LLLLimbo/cedar-ide/actions/runs/38174023016)，Linux 1,818/0/66，Windows 1,761/0/110，两端各 48 套件；15 项 Tab、27 项高度、23 项侧栏、33 项 Search 和 19 项报告键盘检查全部通过。报告进程证明为 15 用例/55 字段/10 请求/两个所有者回收。新鲜精确包 Linux GUI 验证真实拖动、鼠标及键盘重置、37 次真实 Tab 后进入编辑器与紧接的 Undo/Redo，关键步骤 56/57/58 无指针重新聚焦，步骤 55 的预期文件名不能替代证据。未 Save、Run、启动 LSP 或修改项目磁盘文件；不声称原始批次、原生请求数、agent 回收或 Windows GUI 验收。原始 0.58.4 待验收报告逐字保存在[归档](TEST_REPORT_PHASE584_TAB_BOUNDARY_FIXTURE.md)，0.58.3 夹具失败与更早失败均保留。

0.59.0 引入显式 Java Parameter info；0.59.1 修正 Python 文本边界及保留能力清单；0.59.2 规范化模拟驱动的自有临时根目录并增加测试诊断，现已完成限定验收。公开提交 `a3f69b469f16b8bd66b2f57e38aaae2bab759281` 的 [CI 38183040039](https://github.com/LLLLimbo/cedar-ide/actions/runs/38183040039) 四项作业和三种包均通过，Linux 1,861/0/67，Windows 1,804/0/111，各 48 套件。两端独立真实 JDT 证明均为三次查询、2/2/0 签名及精确参数高亮，并独立核实其自身请求账本与清理。限定 Linux 原生验证参数/重载、空结果、编辑/光标失效、Dismiss、脏稿选区及 Tab 后立即 Undo/Redo；首次清理 125.452 秒超过原 120 秒上限的失败保留，独立同包补验以 28.175 秒通过。原始 Java 源码未变；JDT 改写生成的 .project 并新增 class，不能声称整个工作区未变。0.59.3 仅增加实际渲染器 Unicode 测试及版本/文档维护，已通过 [CI 38192432380](https://github.com/LLLLimbo/cedar-ide/actions/runs/38192432380) 四项作业与三种包，Linux 1,862/0/67，Windows 1,805/0/111，各 48 套件。不新开 GUI；已核实生产等价审查承接 0.59.2 原生证据及其限制，当前工作区未保有原始私有原生附件，不声称重新查看。Windows GUI、原生请求数/agent 回收、延迟响应竞态和广泛 Unicode/IME 仍未声明验收。0.59.0 的 Windows 五项及 Linux 能力清单失败、0.59.1 Windows 六条失败与跳过的生产证明均保留；路径别名解释仍未获运行时确认。协议 4/schema 1/32 项直接能力上限、Maven 分组及共享解析器 32 签名/64 参数/每标签 8 KiB/合计 128 KiB 限额均不变。详见[参数说明](JAVA_PARAMETER_INFO.md)及[当前报告](TEST_REPORT.md)。
