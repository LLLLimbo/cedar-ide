# Cedar IDE · Rust 原生远程开发底座

> Public-source note: named raw logs, screenshots and measurement payloads are omitted from this repository. See [verification evidence](PUBLICATION.md#verification-evidence).

Cedar 是一个可构建、可运行的独立 Rust IDE 工程。目标是降低本地前端负担，并把远程开发作为核心路径。当前是持续开发中的第十一阶段连接取消与资源观测工程（**0.11.0**），**不是 IntelliJ IDEA 的完整替代品**，也不兼容其插件；没有复制 JetBrains 的专有实现或使用其产品标识。

## 已能实际使用

- 原生 egui/Glow 桌面界面，无 WebView、Electron 或前端 JVM
- 本地目录与 SSH 工作区，共用同一文件、搜索、工具和语言服务后端
- **远程能力发现**：缓存一次有效握手，报告后端版本/平台/操作支持；按后端实际能力启用功能，声明不授予执行信任
- 目录浏览、多标签编辑、新建文件、行号、简单 Java/Kotlin/Rust 高亮
- Ctrl/Cmd+P 打开路径、Ctrl/Cmd+F 文件内查找、Ctrl/Cmd+S 保存、Ctrl/Cmd+W 关闭标签
- SHA-256 版本检查；文件在外部改变时拒绝覆盖；原子替换和权限保留
- **磁盘对比与干净标签重载**：显式双栏比较当前草稿与磁盘；只允许干净标签重载，二次读取复核，撤销后保留新的磁盘基线
- 脏标签/退出确认；断线保留当前进程中的草稿；显式重连
- **本地草稿恢复**：后台防抖备份、精确版本确认、启动时查看并显式恢复；保留原始保存版本，恢复后仍拒绝覆盖外部修改
- **异步命令任务**：显式 executable + 字面量参数行、实时有界输出、状态与取消；命令运行时仍能打开、编辑和保存文件
- **保存的命令配置**：显式加载/编辑/保存工作区 `cedar.tasks.json`，保留空参数、中文和 shell 字面量；复用编辑器版本冲突与恢复路径，加载/保存/重连都不自动运行
- 项目文本搜索与跳转；Git、命令和语言服务均要求显式工作区信任
- agent 侧真实 LSP：启动、初始化、文件同步、补全/定义/悬浮请求、诊断事件、停止与重启
- 350ms 防抖自动同步、自动诊断列表、F12 定义跳转、类型悬浮、Ctrl+Space 补全
- **安全格式化预览**：只读 Before / After，显式 Apply / Cancel；校验精确草稿版本后单次撤销事务，不自动保存
- **引用查找**：同步所有匹配语言的打开草稿，可选择包含声明；显示无版本结果的时效边界，跳转由 agent 校验工作区
- **文档大纲**：显式刷新，保留层级或平面符号结果，按声明选择范围导航；编辑后失效
- 补全可延迟解析 import，校验所有编辑后一次性改动草稿；一次撤销/重做覆盖整个操作；不执行服务器返回的任意命令
- 中文注释/字符串可按需加载现有系统 CJK 字体，无静默下载或打包系统字体
- 独立 DAP 子进程传输已在第二阶段用真实 Python 调试器验证断点/栈/变量；尚未接入 IDE 调试界面或远程代理

## 构建与运行

需要 Rust **1.99 或更新版本**。该版本用 rustc 1.99.0 构建和检查，Cargo.lock 固定依赖版本。

```sh
cargo build --release --workspace --locked
cargo run -p cedar-app --bin cedar -- examples/demo
```

Windows 使用 Visual Studio C++ Build Tools / MSVC Rust 工具链；本机完成构建后，可执行文件位于 `target\release\cedar.exe`，本地连接还要求同目录的 `cedar-agent.exe`；不要只复制前端。缺失或损坏的 agent 会报错，不搜索 PATH 或回退到进程内执行。Linux 需要桌面会话及 OpenGL、X11 或 Wayland 运行时。首次构建需要下载 crates.io 依赖。

公开仓库 [LLLLimbo/cedar-ide](https://github.com/LLLLimbo/cedar-ide) 的 0.9.0 提交 [`270119b45eea1d37581a497e1bbe9a2d4ba3764a`](https://github.com/LLLLimbo/cedar-ide/commit/270119b45eea1d37581a497e1bbe9a2d4ba3764a) 已通过[同提交 Ubuntu/Windows CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37727806345)：正常 Windows 隔离 agent 的专用 Java/JDT 路径与真实编辑器事务通过；Stop 如实报告超时后的强制清理，不冒充自然退出。0.10.0 显式、只读的[断线保存核对](docs/INTERRUPTED_SAVES.md)已通过[精确提交双平台 CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37736569381)，两端九项实际进程故障用例全部执行通过；保留后续输入和 Undo，不自动重放写入。0.11.0 增加有界[连接与读取取消](docs/CONNECTION_CANCELLATION.md)，尚待精确提交 CI。**Windows Git、同步 Run 和通用 LSP 仍不启用**。Java 配置见[使用说明](docs/WINDOWS_JAVA_SETUP.md)；原生 GUI 与真实 SSH 仍待独立验收，当前证据见[测试报告](docs/TEST_REPORT.md)。

## 磁盘对比与重载

在当前文件路径旁点击 **Compare with disk**，查看当前草稿和本次读到的磁盘内容。**Refresh** 显式重新读取，**Close** 关闭审阅；这些操作不写项目文件，也不要求工具执行信任。

只有未修改的干净标签可点击 **Reload clean tab**。接受前会再次读取；磁盘又有变化时只更新预览，须重新确认。成功后新的磁盘内容成为保存基线，单次 Undo 可回到旧文字并标记为未保存，Redo 回到新内容。脏稿、过期请求、标签切换或同一帧的新输入都不能被旧结果替换。恢复副本仍受原有所有权保护。此阶段提供手动审阅，没有自动文件监听或合并，详见[磁盘审阅边界](docs/DISK_REVIEW.md)。

## 格式化、引用与大纲

可信工作区中启动已安装的语言服务器，选择匹配当前文件的语言配置；相应按钮只在服务器声明支持时可用。

- **Format preview**：选择缩进宽度（1–16）和空格/Tab，先同步当前草稿，再请求格式化。预览不改动文字；**Apply** 再次校验来源后只修改内存草稿，保存仍需 Ctrl/Cmd+S。Cancel、Escape 或关闭预览会放弃提案
- **Find references**：先同步所有匹配语言的已打开草稿，再按 **Include declaration** 查找。结果标为无版本服务器快照；不能保证未打开文件或查询后变动的目标仍然新鲜。跳转复用已有脏标签，不覆盖其文字
- **Refresh outline**：一次性刷新当前文档大纲。层级结果保留类/方法嵌套，平面结果保持平面；点击使用 selectionRange，平面结果的 URI 仍须通过 agent 边界检查

输入、标签切换、关闭重开、重连、语言服务重启或更新请求都会使旧提案失效。单纯移动光标不使格式化过期，Apply 会重新映射最新光标。畸形、重叠、越界、无效 UTF-16/CRLF 或超限编辑整体拒绝；空或等效结果不制造撤销记录。不执行服务器命令、WorkspaceEdit 或文件重命名。

真实 JDT 已通过双未保存文档引用、类/方法大纲、中文格式化、旧版本拒绝、幂等性和磁盘源码不变检查。原生候选窗口已操作预览、Apply/Cancel、立即撤销/重做和导航；最终 release 窗口又验证 Escape 取消、移动光标/切换文件后的单步撤销与重做，两份项目源码磁盘逐字不变。上述原生语言界面结果是第四阶段历史证据；第五阶段共享测试脚本改动后又通过了真实 JDT 经 stdio agent 的 10 项检查，见[JDT 复验记录](PUBLICATION.md#verification-evidence)，不等于重新验收整个原生语言界面。详见 [交互与边界](crates/app/PHASE4_LANGUAGE.md)、[文本编辑校验](docs/TEXT_EDITS.md) 和 [重构安全路线](docs/REFACTORING_ROADMAP.md)。

## 草稿恢复

每次编辑器会话默认开启恢复。页脚展示恢复状态，点击页脚或 **Recovery** 可查看恢复位置、问题和已有副本。草稿经过一秒尾部防抖后写入**前端电脑的本地存储**，SSH 文件的副本也保存在前端。只有当前精确草稿版本完成存储确认，才显示 **Draft backed up locally**；排队、正在保存或旧版本确认都不代表最新文字已备份。

重启时先 **Review restore**，再显式选择 **Connect with trust off and restore**。恢复不写项目文件、不启用执行信任、不启动命令或语言服务，且保留原始 base revision；恢复后的 Ctrl+S 仍会检测文件冲突。保存/丢弃只清理该标签拥有的恢复副本，未查看的旧副本不会被新打开的磁盘版本静默覆盖。明确丢弃并退出时，会等待相应恢复副本删除确认。

恢复副本包含草稿和上次保存文本，采用本地明文存储，可能含有源码中的秘密。Unix 使用私有目录/文件权限；Windows 继承 ACL，未承诺同等隐私保证。可通过绝对路径 `CEDAR_RECOVERY_DIR` 指定位置，也可在 Recovery 中关闭本次会话的后续备份；关闭不会删除已有副本。默认路径、配额、锁、损坏处理和平台边界见 [恢复设计](docs/RECOVERY.md)。

Linux 已验证：收到精确备份确认后强制结束进程，重启可恢复原样中文草稿，并继续拒绝覆盖外部修改。**最近一次确认之后的输入，包括防抖窗口内的输入，仍可能丢失；没有断电、硬件或所有平台的零丢稿保证。** 恢复不是项目自动保存或版本控制。

## 命令任务

**Commands** 接收独立 executable 和有序字面量参数行，例如 executable 为 `cargo`、两行参数分别为 `test` 和 `--workspace`。参数默认不经过 shell 解释；主动选择 `sh`/`-c` 则是主动运行 shell。标准输入关闭，没有交互式终端或 PTY。

**Load** 显式读取工作区根目录的 `cedar.tasks.json`；已打开的配置以当前编辑器草稿为准。可选择配置、**New profile**、增删参数行，并在执行前查看转义后的 executable/argv 与目标工作区。**Save profile** 将完整验证后的 JSON 作为一次编辑器撤销事务，再请求普通的 SHA-256 版本检查保存；**Run** 不隐式保存。加载、选择、保存、恢复和重连都不运行命令，也不存储或启用执行信任。信任关闭时仍可编辑和保存配置。

配置限制为 256 KiB / 32 个命令，每条最多 256 个参数、合计 64 KiB。严格拒绝未知/重复字段、重复名称和不支持的版本；不提供 autorun、环境变量、工作目录覆盖、秘密或变量替换字段。原始编辑器与表单冲突时停止 Save/Run 并保留两份草稿；同工作区重连后须显式复核或 Load。表单修改在序列化前只存在当前会话，关闭时有丢弃确认。完整格式、恢复和冲突边界见[命令配置](docs/TASK_PROFILES.md)。

每个连接同时运行一个异步命令，超时为 1–300 秒，stdout/stderr 各上限 256 KiB。UI 最快每 250ms 轮询一次完整有界快照，区分 Running、Cancelling、Succeeded、Failed、Cancelled、Timed out 等状态。点击 Cancel 只是请求取消，仍须等待终态；自然结束可能先发生。运行中关闭或重连会要求 **Cancel-and-wait**，终态后再重试原操作。断线造成结果不明时明确提示，绝不自动重跑命令。

Linux 已有真实子进程、独立 agent 和原生界面验证；macOS 未运行。Windows Local 通过同目录的独立 agent 运行异步任务，要求明确的原生 `.exe` 绝对路径，例如 `C:\Tools\cargo.exe`；不搜索 PATH/PATHEXT、不补扩展名、不接受 `.bat`/`.cmd` 包装文件。Windows 任务保留完整 u32 退出码并显示十进制与十六进制，默认进程内后端仍拒绝执行。Windows 前端也可使用支持该功能的远程 agent。Windows 新接入须独立验收，进程组/Job 清理不是恶意代码沙箱。完整状态、界限和清理保证见 [命令任务设计](docs/RUN_TASKS.md)与[Windows 进程边界](docs/WINDOWS_PROCESSES.md)。

## 远程开发

1. 在远程 Linux 机器构建 `cargo build --release -p cedar-agent --locked`
2. 将 `cedar-agent` 放到自己选择的远程目录；当前不自动上传或安装
3. 在系统终端完成 SSH 登录和服务器指纹核验。前端只使用已有 OpenSSH 配置/凭据
4. 启动 Cedar，选择 **Remote over SSH**，填写 `user@host` 或 SSH 配置别名、端口、远程绝对目录、agent 可执行路径
5. 如需 Git、构建命令或语言服务，只对可信工作区启用工具执行

前端与 agent 握手要求**协议版本恰好为 4**；新增有界 `agent` 元数据报告产品版本、平台与支持的操作，不用产品版本推断协议兼容。0.6.0 客户端对旧的无元数据协议 4 agent 保留目录、读取、保存和搜索；Git、命令和语言服务须升级 agent 后重新连接。这是明确的兼容策略收紧，不会回退到同步命令。显式空能力列表不等于旧 agent；畸形元数据直接拒绝。协议 3 或 5 仍被拒绝，建议前后端部署同一已验证检查点。详见[能力边界](docs/REMOTE_CAPABILITIES.md)。

连接通过 `ssh -T` 的标准输入/输出传输有界 JSON，agent 不开启 TCP 监听。即使 localhost 也要求严格主机密钥检查；禁用 agent/X11/端口/隧道转发、本地命令、复用连接与自动记录主机密钥。覆盖可能关闭 stdin、后台化或禁止命令会话的 SSH 配置。仅为旧客户端忽略三个新增选项别名，安全开关不会被忽略；参数设计最低支持 OpenSSH 7.6，但旧版本运行待验收。用户的 ProxyJump/ProxyCommand 路由仍受支持，不把 SSH 配置当作沙箱；不自动信任主机、不生成密钥、不保存密码。

SSH 远程 shell 目前要求 POSIX；Windows 前端可连接 Linux 远程工作区，本地 Windows 则使用捆绑的 stdio agent。远程 JDK/语言服务器和命令运行在服务器端。已验证真实 stdio agent 的文件、LSP、异步任务及配置协议链，并以真实管道覆盖超时、坏帧、错误响应 ID、stderr 洪泛、失去写入确认和子进程退出。客户端后台回收器给予直接子进程最多两秒的正常退出机会，再尝试终止并回收；这不保证远程进程或逃逸后代已清理。**尚未通过真实 SSH 服务器完成认证连通性或断网恢复测试**；临时认证测试仍等待专门批准，没有创建测试密钥或服务器。重连不接管旧 agent 的任务 ID，也不重跑不明结果的命令。远程仍是核心验收要求，具体证据与下一道门槛见[远程验证](docs/REMOTE_VALIDATION.md)。

## 信任与平台边界

- Git status 也可能通过仓库配置触发过滤器，所以与命令、语言服务器一起要求显式信任
- 工作目录和进程组不是 OS 沙箱；可信工具拥有执行账户的权限，恶意自行脱离进程组的程序可能继续运行
- Windows 异步命令仅允许隔离 agent 后端；Git、同步 Run 和语言服务器启动仍禁用。Windows Local 的普通编辑也依赖同目录 agent；真实 Windows/macOS GUI 与恢复交互仍待验收
- 路径访问拒绝绝对路径、越界和符号链接；无法抵抗所有恶意并发文件系统修改
- 保存会做晚期版本复核，但无法对不合作的外部写入者提供文件系统级原子 compare-and-swap
- 普通文件的 Windows 替换路径已改用 Rust 1.99 的 `std::fs::rename`，兼容 delete-sharing 读句柄；不绕过只读或共享限制、不先删除目标、不自动重试。第五阶段同提交真实 Windows CI 已通过该回归，Windows GUI 仍未验收，见[保存边界](docs/WORKSPACE_SAVE.md)
- 恢复存储错误、锁冲突、配额满或损坏会显示问题，不阻止继续编辑，不静默删除旧副本腾空间，也不虚报备份成功
- 外部语言服务器可能需要 JVM，并会自行索引/执行构建探测；Rust 前端不意味着整个 Java/Kotlin 工具链不使用 JVM
- 第二阶段检查的官方 Kotlin 包要求新的 EULA，且缺少其引用的 EULA 文件，未接受任何协议；已验证的社区替代版本已弃用，不能据此声称支持当前 Kotlin 全特性

## 当前资源策略

按需重绘，无常驻全项目索引；I/O 与工具执行不阻塞 UI；单文件后端上限 1 MiB、编辑标签上限 32、协议帧上限 8 MiB；搜索、进程输出、语言事件和恢复队列有界。大文本降级为无高亮显示。项目代码不发送到云模型或分析服务；恢复副本留在前端本地，用户自选同步目录等外部分享行为不在此保证内。

这些是架构措施，不等于“比 IDEA 节省 X%”。目前没有在同一机器、项目与功能集下完成 IDEA 对照基准。第三阶段历史记录包含一次 112.597 秒 release 前端小样例：CJK、默认恢复开启、信任关闭且无 JVM，前端观察到的 HWM 为 117.97 MiB。它与第二阶段交互不同，不能证明恢复开销、内存改善或 IDEA 相对优势。前端与 Java/Kotlin JVM 读数分别报告，不能拼成同时测得的总占用。0.11.0 增加同一次真实 Java 验收中的进程树观测，分别记录无界面测试驱动、agent 与 JVM；这是带构建类型和采样完整性标记的基线，尚待精确提交原生 CI，不能代替 GUI 总占用或 IDEA 对照。详见 [进程树基线](docs/RESOURCE_BASELINE.md)、[资源说明](docs/PERFORMANCE.md) 与 [第五阶段测试报告](docs/TEST_REPORT_PHASE5.md)。

## 验证

第九阶段 A 的基础层变更与本地检查见[当前报告](docs/TEST_REPORT.md)，新增 Windows 行为仍需精确提交 CI 实际运行。

第八阶段最终通过 **521 项 Rust 测试（515 + 6 显式）**、五条 agent 黑盒链、两项导出回归、严格 Linux/MSVC Clippy、Linux release 与 release-agent 集成。最终原生 Trust 关闭窗口已复验磁盘比较、二次读取竞态、重载后反复移动光标的 Undo/Redo 与脏稿恢复；范围见[第八阶段报告](docs/TEST_REPORT_PHASE8.md)。0.8.0 的精确公开提交也已通过 Ubuntu/Windows CI，包括全部 13 + 7 项 Windows 生命周期/agent 测试；第九阶段不能继承旧二进制的验收结果。
```sh
bash scripts/verify.sh
```

第五阶段最终聚合通过 fmt、严格 Clippy、**413 个普通 Rust 测试 + 6 个显式独立进程测试（共 419）**，以及文件、LSP、异步任务、命令配置四条 Python 黑盒链。聚合有 9 个 opt-in 忽略，其中 6 个进程测试随后显式运行；前端 227 项包含在总数中。最终 Linux release 构建与 Windows MSVC 全目标编译检查通过，且随后通过了同提交真实 Ubuntu/Windows CI，详见[第五阶段报告](docs/TEST_REPORT_PHASE5.md)。

第五阶段最终 Linux release 在连接身份修复后，已重新验证信任关闭下的首次配置 Load、编辑器激活、美化 JSON 保存、精确参数和外部冲突保护；记录于阶段报告。冒号拼接工作区标识的碰撞已改为分字段枚举，并有 7 项无网络回归防止错误端点接管脏稿。环境更换后保留了对应报告，公开树不含这些原始截图。原生 Run/Cancel/重连复核仍需要单独批准启用测试工作区信任；真实认证 SSH 与 Windows/macOS GUI 也尚未验收。

历史验收与资源读数按阶段保存：[第四阶段](docs/TEST_REPORT_PHASE4.md)、[0.3.1 恢复修复](docs/TEST_REPORT_HOTFIX_0_3_1.md)、[第三阶段](docs/TEST_REPORT_PHASE3.md)、[第二阶段](docs/TEST_REPORT_PHASE2.md)、[第一阶段](docs/TEST_REPORT_PHASE1.md)。第五阶段公开精确提交的 Ubuntu/Windows CI 也已通过；所有历史报告保留封存时的描述，后续结果另列，不回写历史。

第六阶段最终聚合通过 **459 项 Rust 测试（453 + 6 显式）**、五条 Python agent 链和两项公开导出回归，Linux release 与 Windows MSVC 全目标严格 Clippy 通过。最终原生 release 已验证能力提示与执行信任分离、Run/LSP 启动保持禁用、未信任文件读写保存可用。准确范围和待验收项见[第六阶段报告](docs/TEST_REPORT_PHASE6.md)。

## 工程结构

| crate | 职责 |
|---|---|
| cedar-app | 原生界面、编辑状态、异步工作队列、LSP / Recovery / Commands 与命令配置 UI |
| cedar-client | 本地与 SSH/子进程传输，握手、超时和断开状态 |
| cedar-protocol | 有界、版本化的工作区 JSON 消息 |
| cedar-workspace | 文件边界、保存、搜索、可信工具、语言服务和异步任务代理 |
| cedar-agent | 远程 stdio 入口 |
| cedar-recovery | 前端本地草稿存储、锁、完整性校验、原子替换和配额 |
| cedar-tasks | 有界异步命令监督、输出捕获、取消和普通子进程组清理 |
| cedar-language | LSP 客户端/生命周期/有界 JSON-RPC；DAP 基础封包 |
| cedar-debugger | 有界异步 DAP 进程传输，尚未接入 UI 或远程 agent |
| cedar-winprocess | Windows Job/句柄/异步管道基础库；7A独立验收，7B起由隔离agent任务监督器使用；9A增加可取消stdin基础层 |

更多：[功能矩阵](docs/FEATURE_MATRIX.md)、[架构](docs/ARCHITECTURE.md)、[语言服务](docs/LANGUAGE_SERVICES.md)、[Kotlin 验证](docs/KOTLIN_VALIDATION.md)、[调试](docs/DEBUGGING.md)、[前端恢复与命令交互](crates/app/RECOVERY_AND_COMMANDS.md)。第五阶段增加显式命令配置、普通文件 Windows 替换修复与远程故障边界验证；第六阶段增加协议 4 内的后端能力发现与单次握手快照；7A 已完成 Windows 进程基础库的真实 CI 验收；7B 接入隔离 agent 的异步任务，并为这层接入单独验证传输故障、后代清理与本地 bundle。第八阶段新增显式磁盘双栏审阅和只对干净标签的二次读取重载，保留撤销、草稿恢复与语言同步边界。格式化、引用与大纲的安全边界见 [重构路线](docs/REFACTORING_ROADMAP.md)。多文件重命名仍暂缓，先解决无版本跨文件结果、跨文档撤销与文件资源操作遗漏的安全问题。完整重构、项目模型和大量 IDEA 功能仍待实现。

## 打包已验证检查点

`scripts/package_checkpoint.py` 可将干净、HEAD 有精确 tag 的源码与已构建的 Linux `cedar` / `cedar-agent` 打成 ZIP，并记录 source commit/tag、二进制 SHA-256 和平台限制。先完成验证、release 构建以及源码/证据提交与 tag，再运行：

```sh
python3 scripts/package_checkpoint.py --output /absolute/path/cedar-checkpoint.zip
```

它不会构建、签名或发布，也不证明源码与预先构建二进制的可复现对应；二进制为未签名 Linux 检查点，不含 Windows exe、JDK 或语言/调试服务器。

## 许可

本项目代码按 MIT 或 Apache-2.0 双许可提供。依赖保留各自许可；参见 `THIRD_PARTY_NOTICES.md`。没有打包 JDK、JDT LS、Kotlin 服务器或 JetBrains 组件。源码包不含下载缓存、临时凭据、用户项目或大型构建目录。

Public source history and omitted machine-specific evidence are described in [PUBLICATION.md](PUBLICATION.md).
