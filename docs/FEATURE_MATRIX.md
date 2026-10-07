# 功能矩阵与后续验收 · phase 5 / 0.5.0

> Public-source note: named raw logs, screenshots and measurement payloads are omitted from this repository. See [verification evidence](../PUBLICATION.md#verification-evidence).

“已实现”指当前代码中有可执行路径；不等于所有平台、真实 SSH 或生产项目均已验收。当前前端/agent 协议仍为 4，没有应用版本或通用能力协商。第五阶段尚未发布，运行证据与待完成检查见[第五阶段报告](TEST_REPORT_PHASE5.md)。

| 领域 | 当前实现 | 尚缺 / 下一阶段验收 |
|---|---|---|
| 原生前端 | egui/Glow、主题、窗口、快捷键；Linux 历史窗口验收；本阶段信任关闭下的 debug/release 配置候选窗口检查 | 最终构建复验、可访问性、完整 IME/无障碍、高 DPI 与 Windows/macOS 运行验收 |
| 编辑器 | UTF-8、多标签、保存、基本高亮、查找、行号、按需系统 CJK 字体回退、预览后显式格式化 | rope/增量语法树、超大文件、块选择、代码折叠、差量/持久撤销 |
| 文件与工作区 | 单根目录、相对路径、按需列表、新建文件；晚期 SHA 冲突复核；普通文件 Windows 替换修复及专用回归 | 本阶段真实 Windows 替换运行验收；文件移动/删除、监听、多根、最近项目、完整会话持久化；非原子 compare-and-swap |
| 防丢稿 | 版本冲突拒写、关闭确认、断线保留；本地防抖恢复、精确版本确认、启动审阅/显式恢复、保留原始 base revision | 防抖后未确认输入仍可丢失；非项目自动保存；无断电保证；三方合并、外部变动提示、真实多平台恢复验收 |
| 恢复隐私 | 前端本地明文、Unix 私有权限、单写者锁、完整性校验、有界配额、错误可见；不恢复执行信任 | 无加密/秘密保险箱；Windows 原生恢复和 ACL 隐私验收未完成；无恶意同账户写入者隔离；无静默淘汰旧稿 |
| 远程 | 系统 OpenSSH、stdio agent、统一后端、显式重连；有界传输故障测试；后台直接子进程回收；SSH 配置硬化 | **真实 SSH 认证/断网互操作仍是核心门槛**；自动部署/升级、版本/能力协商、断线事务恢复、旧任务接管、端口管理；旧 OpenSSH 客户端运行验收 |
| Java/Kotlin | 文本编辑、语法着色、JDT LS 诊断/补全/跳转；第二阶段真实 Java 与旧版社区 Kotlin 语义链验证 | 完整 Maven/Gradle 工程模型、SDK 管理、当前官方 Kotlin 许可/兼容性验证、生产项目回归 |
| LSP / 重构 | 自动同步、诊断、悬浮/定义、补全与延迟 import；只读格式化预览、显式 Apply/Cancel、单文档撤销；同步匹配草稿后查引用；显式层级/平面大纲；有界队列/超时 | 引用是无版本结果，不保证目标时效；多文件重命名需先解决快照/资源操作/跨文档撤销安全；inline diagnostics、代码操作、snippets、服务多路化 |
| 调试 | 独立异步 DAP 传输；第二阶段真实 Python 断点/栈/变量/继续/停止验证 | IDE 调试 UI/远程桥接、可靠后代进程回收、监听安全、Java/JVM 调试 |
| 命令运行 | 明确 executable + 字面量 argv；RunStart/Poll/Cancel、实时有界输出、终态区分、运行中继续编辑保存、关闭/重连保护 | 本阶段原生显式 Run/Cancel/重连复核待批准启用信任；交互式 PTY、测试结果树、并行任务、Windows Job Objects；macOS 运行未验收 |
| 保存的命令配置 | 显式 Load/选择/新建/Save；`cedar.tasks.json` 严格有界格式；字面量参数行与预览；普通编辑器版本检查、单次撤销和恢复；重连显式复核 | 不是完整运行/调试配置系统；无自动发现、预设、环境变量、目录覆盖、变量展开或 autorun；未序列化表单只在当前会话 |
| 任务安全 | 配置操作零自动执行；每连接一个异步任务、1–300 秒、每流 256 KiB、8 个历史记录；Linux 普通进程组清理；不自动重试不明结果 | 非 OS 沙箱，恶意逃逸后代可能存活；强杀 agent 不保证任务清理；遗留同步 Run/Git/LSP/DAP 不计入异步任务限额 |
| 搜索 | 有界文本搜索、结果跳转；语言服务引用查找与工作区边界导航 | 正则、替换、全工作区符号索引 |
| Git | 可信工作区状态 | diff/hunks/stage/commit/blame/merge、分支/远端管理 |
| 插件 | Rust crate 扩展边界 | 稳定插件 ABI/协议、权限、生命周期、市场；无 IDEA 插件兼容承诺 |
| 企业功能 | 无 | 数据库、Spring、Web、容器、应用服务器、Profiler、协作等需分别设计 |
| 性能 | 懒加载、按需重绘、有界读取/输出/传输/恢复存储 | 无本阶段新内存基准；历史短时前端读数与 JVM 分开；同项目可复现基线、远程延迟、长会话泄漏、生产项目回归、完整进程树核算 |
| 分发 | Cargo 工程、锁文件、许可清单、公开分阶段源码；第四阶段精确提交 Ubuntu/Windows CI 已通过；本阶段最终聚合、release 构建与 Windows 目标编译通过 | 本阶段发布/精确提交 CI；Windows/macOS 原生 GUI、已签名安装包、自动升级与安全发布 |

## 本阶段验收边界

- 最终 `scripts/verify.sh` 通过：fmt、严格 Clippy、413 个普通 Rust 测试 + 6 个显式进程测试（共 419）、四条 Python 黑盒链。聚合有 9 个 opt-in 忽略，其中 6 个进程测试随后显式执行；前端 227 通过、2 个 opt-in 忽略，已包含在总数中
- 最后发现并修复了冒号拼接连接标识的字段碰撞：改用 `WorkspaceKey` 分字段枚举，7 项无网络回归覆盖脏稿跨端点保护、规范根目录变化和仅信任设置变化的边界；均进入最终聚合
- Linux 原生候选窗口在信任关闭下验证显式 Load、配置选择、字面量参数、Save 和外部改动冲突。随后 release 候选复验通过首次 Load 激活编辑器、美化 JSON 和精确参数落盘，但早于最终连接身份修复；未完成原生脏窗口关闭验收。Run/Cancel/重连的信任开启尚待批准
- 真实管道故障测试和独立 agent 测试覆盖协议不匹配、坏帧/错误 ID、超时、失去写入确认、有界 stderr、正常 EOF、BrokenPipe 与任务清理；强杀 agent 的反例明确证明不能据此保证远程清理
- OpenSSH 10.0p2 的 `ssh -G` 已接受本地生成参数，不建立连接。未创建认证测试密钥、服务器或监听器；真实认证与断网结果尚不存在
- Windows 目标编译/严格 Clippy 通过不等于 Windows 运行。普通文件替换的新路径和可移植故障测试必须由本阶段精确提交的真实 Windows CI 验收
- 第四阶段公开提交 [`b1ad6f53e325bf19b1fec469394f6eb6ecf2c3ca`](https://github.com/LLLLimbo/cedar-ide/commit/b1ad6f53e325bf19b1fec469394f6eb6ecf2c3ca) 的 [Ubuntu/Windows CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37630631194) 均成功；其原始[阶段报告](TEST_REPORT_PHASE4.md)保留当时的待 CI 描述，不回写历史
- 本阶段真实 JDT 经 stdio agent 的 10 项检查已重新通过，见[JDT 记录](../PUBLICATION.md#verification-evidence)；原生语言界面、Kotlin/debugpy、恢复原生验收和资源读数仍是历史证据。Windows CI 核心测试也不是原生 GUI 验收

## 建议下一顺序

1. 完成本阶段源码检查点与精确提交 Ubuntu/Windows CI，并补齐最终原生配置验收；本地聚合已通过，后续修复仍须重跑受影响项，保留候选与最终证据的区别
2. 远程核心路径：获得狭窄测试批准后验证真实 SSH 认证、严格主机密钥、远程路径引用、断开/重连和不明结果不重放；明确版本/能力边界、实际进程清理和 Windows 前端到 Linux 后端的证据
3. Windows Job Object 与可取消管道、外部文件监听/冲突体验、多平台编辑与恢复；支持未完成前保持 Windows 本地命令/Git/LSP 禁用
4. DAP 调试 UI/agent 与真实程序闭环；先解决监听安全和普通后代进程回收，再扩展 PTY、测试树与 JVM 调试
5. Java/Kotlin 工程导入：真实大型 Maven/Gradle 项目、JDK 选择与索引进度；解决当前官方 Kotlin 许可/安装阻碍后再验兼容性
6. 安全重构：保留严格 URI、UTF-16、版本与脏稿边界；多文件重命名须先解决无版本跨文件快照、跨文档撤销和遗漏文件资源操作，见[重构路线](REFACTORING_ROADMAP.md)
7. Git/插件协议与框架工具，按实际需求逐项实现；在等价功能集下建立可复现性能基线

上述路线不是已经实现的功能或交付承诺。完成后仍不能直接宣称与 IntelliJ IDEA 全功能等价，需要持续维护兼容矩阵、真实测量与长期项目使用反馈。
