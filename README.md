# Cedar IDE · Rust 原生远程开发底座

Cedar 是一个可构建、可运行的独立 Rust IDE 工程。目标是降低本地前端负担，并把远程开发作为核心路径。当前是持续开发中的第三阶段工程（**0.3.0**），**不是 IntelliJ IDEA 的完整替代品**，也不兼容其插件；没有复制 JetBrains 的专有实现或使用其产品标识。

## 已能实际使用

- 原生 egui/Glow 桌面界面，无 WebView、Electron 或前端 JVM
- 本地目录与 SSH 工作区，共用同一文件、搜索、工具和语言服务后端
- 目录浏览、多标签编辑、新建文件、行号、简单 Java/Kotlin/Rust 高亮
- Ctrl/Cmd+P 打开路径、Ctrl/Cmd+F 文件内查找、Ctrl/Cmd+S 保存、Ctrl/Cmd+W 关闭标签
- SHA-256 版本检查；文件在外部改变时拒绝覆盖；原子替换和权限保留
- 脏标签/退出确认；断线保留当前进程中的草稿；显式重连
- **本地草稿恢复**：后台防抖备份、精确版本确认、启动时查看并显式恢复；保留原始保存版本，恢复后仍拒绝覆盖外部修改
- **异步命令任务**：显式 executable + JSON argv、实时有界输出、状态与取消；命令运行时仍能打开、编辑和保存文件
- 项目文本搜索与跳转；Git、命令和语言服务均要求显式工作区信任
- agent 侧真实 LSP：启动、初始化、文件同步、补全/定义/悬浮请求、诊断事件、停止与重启
- 350ms 防抖自动同步、自动诊断列表、F12 定义跳转、类型悬浮、Ctrl+Space 补全
- 补全可延迟解析 import，校验所有编辑后一次性改动草稿；一次撤销/重做覆盖整个操作；不执行服务器返回的任意命令
- 中文注释/字符串可按需加载现有系统 CJK 字体，无静默下载或打包系统字体
- 独立 DAP 子进程传输已在第二阶段用真实 Python 调试器验证断点/栈/变量；尚未接入 IDE 调试界面或远程代理

## 构建与运行

需要 Rust **1.99 或更新版本**。该版本用 rustc 1.99.0 构建和检查，Cargo.lock 固定依赖版本。

```sh
cargo build --release --workspace --locked
cargo run -p cedar-app --bin cedar -- examples/demo
```

Windows 使用 Visual Studio C++ Build Tools / MSVC Rust 工具链；本机完成构建后，可执行文件位于 `target\release\cedar.exe`。Linux 需要桌面会话及 OpenGL、X11 或 Wayland 运行时。首次构建需要下载 crates.io 依赖。

第三阶段已做 Linux 全工作区 release 构建、测试和实际窗口交互，以及 Windows MSVC 目标的全工作区、全 targets/features 编译检查。**未在 Windows 运行或完成 Windows 最终链接，也没有交付经过 Windows 运行验证的 exe**。仓库附带 Linux/Windows CI 配置，但它尚未在外部服务运行。

## 草稿恢复

每次编辑器会话默认开启恢复。页脚展示恢复状态，点击页脚或 **Recovery** 可查看恢复位置、问题和已有副本。草稿经过一秒尾部防抖后写入**前端电脑的本地存储**，SSH 文件的副本也保存在前端。只有当前精确草稿版本完成存储确认，才显示 **Draft backed up locally**；排队、正在保存或旧版本确认都不代表最新文字已备份。

重启时先 **Review restore**，再显式选择 **Connect with trust off and restore**。恢复不写项目文件、不启用执行信任、不启动命令或语言服务，且保留原始 base revision；恢复后的 Ctrl+S 仍会检测文件冲突。保存/丢弃只清理该标签拥有的恢复副本，未查看的旧副本不会被新打开的磁盘版本静默覆盖。明确丢弃并退出时，会等待相应恢复副本删除确认。

恢复副本包含草稿和上次保存文本，采用本地明文存储，可能含有源码中的秘密。Unix 使用私有目录/文件权限；Windows 继承 ACL，未承诺同等隐私保证。可通过绝对路径 `CEDAR_RECOVERY_DIR` 指定位置，也可在 Recovery 中关闭本次会话的后续备份；关闭不会删除已有副本。默认路径、配额、锁、损坏处理和平台边界见 [恢复设计](docs/RECOVERY.md)。

Linux 已验证：收到精确备份确认后强制结束进程，重启可恢复原样中文草稿，并继续拒绝覆盖外部修改。**最近一次确认之后的输入，包括防抖窗口内的输入，仍可能丢失；没有断电、硬件或所有平台的零丢稿保证。** 恢复不是项目自动保存或版本控制。

## 命令任务

可信工作区中的 **Commands** 接收独立 executable 和字面量 JSON argv，例如 executable 为 `cargo`、argv 为 `["test", "--workspace"]`。参数默认不经过 shell 解释；主动选择 `sh`/`-c` 则是主动运行 shell。标准输入关闭，没有交互式终端或 PTY。

每个连接同时运行一个异步命令，超时为 1–300 秒，stdout/stderr 各上限 256 KiB。UI 最快每 250ms 轮询一次完整有界快照，区分 Running、Cancelling、Succeeded、Failed、Cancelled、Timed out 等状态。点击 Cancel 只是请求取消，仍须等待终态；自然结束可能先发生。运行中关闭或重连会要求 **Cancel-and-wait**，终态后再重试原操作。断线造成结果不明时明确提示，绝不自动重跑命令。

Linux 已有真实子进程、独立 agent 和原生界面验证；macOS 未运行。Windows 本地命令执行暂时禁用，等待 Job Object 和可取消管道实现；Windows 前端可使用支持该功能的远程 agent。进程组清理不是抵抗恶意进程逃逸的沙箱。完整状态、界限和清理保证见 [命令任务设计](docs/RUN_TASKS.md)。

## 远程开发

1. 在远程 Linux 机器构建 `cargo build --release -p cedar-agent --locked`
2. 将 `cedar-agent` 放到自己选择的远程目录；当前不自动上传或安装
3. 在系统终端完成 SSH 登录和服务器指纹核验。前端只使用已有 OpenSSH 配置/凭据
4. 启动 Cedar，选择 **Remote over SSH**，填写 `user@host` 或 SSH 配置别名、端口、远程绝对目录、agent 可执行路径
5. 如需 Git、构建命令或语言服务，只对可信工作区启用工具执行

前端与 agent 必须同版本：**0.3.0 使用协议版本 3**，会拒绝不兼容的旧版 agent。此次协议增加 `RunStart` / `RunPoll` / `RunCancel`。

连接通过 `ssh -T` 的标准输入/输出传输有界 JSON，不开启服务端 TCP 监听。开启严格主机密钥检查，禁用 agent/X11/端口转发，不自动信任主机、不生成密钥、不保存密码。

SSH 远程 shell 目前要求 POSIX；Windows 前端连接 Linux 远程工作区是主要 Windows 路径。远程 JDK/语言服务器和命令运行在服务器端。已验证真实 stdio agent 的文件、LSP 和异步任务协议链，以及 SSH 参数/引用规则；**尚未通过真实 SSH 服务器完成认证连通性或断网恢复测试**。重连不接管旧 agent 的任务 ID，也不重跑不明结果的命令。

## 信任与平台边界

- Git status 也可能通过仓库配置触发过滤器，所以与命令、语言服务器一起要求显式信任
- 工作目录和进程组不是 OS 沙箱；可信工具拥有执行账户的权限，恶意自行脱离进程组的程序可能继续运行
- Windows 本地命令、Git 和语言服务器启动暂时禁用；普通本地编辑不受影响，真实 Windows/macOS 编辑与恢复仍待验收
- 路径访问拒绝绝对路径、越界和符号链接；无法抵抗所有恶意并发文件系统修改
- 保存会做晚期版本复核，但无法对不合作的外部写入者提供文件系统级原子 compare-and-swap
- 恢复存储错误、锁冲突、配额满或损坏会显示问题，不阻止继续编辑，不静默删除旧副本腾空间，也不虚报备份成功
- 外部语言服务器可能需要 JVM，并会自行索引/执行构建探测；Rust 前端不意味着整个 Java/Kotlin 工具链不使用 JVM
- 第二阶段检查的官方 Kotlin 包要求新的 EULA，且缺少其引用的 EULA 文件，未接受任何协议；已验证的社区替代版本已弃用，不能据此声称支持当前 Kotlin 全特性

## 当前资源策略

按需重绘，无常驻全项目索引；I/O 与工具执行不阻塞 UI；单文件后端上限 1 MiB、编辑标签上限 32、协议帧上限 8 MiB；搜索、进程输出、语言事件和恢复队列有界。大文本降级为无高亮显示。项目代码不发送到云模型或分析服务；恢复副本留在前端本地，用户自选同步目录等外部分享行为不在此保证内。

这些是架构措施，不等于“比 IDEA 节省 X%”。目前没有在同一机器、项目与功能集下完成 IDEA 对照基准。第三阶段新增一次 112.597 秒 release 前端小样例：CJK、默认恢复开启、信任关闭且无 JVM，前端观察到的 HWM 为 117.97 MiB。它与第二阶段交互不同，不能证明恢复开销、内存改善或 IDEA 相对优势。前端与 Java/Kotlin JVM 读数分别报告，不能拼成同时测得的总占用。详见 [资源说明](docs/PERFORMANCE.md) 与 [测试报告](docs/TEST_REPORT.md)。

## 验证

```sh
bash scripts/verify.sh
```

第三阶段完整通过：格式检查、严格全 targets/features Clippy、244 个普通 Rust 测试、额外 1 个独立 agent 进程测试，以及文件、LSP、异步任务三条 Python 黑盒闭环。聚合套件有 4 个显式 opt-in 测试被忽略，其中 agent 测试由脚本随后单独执行；真实 JDT、系统字体和 debugpy 验证属于第二阶段历史证据，未冒充本轮重测。Windows 可按 CI 的 PowerShell 示例运行对应 Cargo 与进程测试。

原生 Linux 窗口的崩溃恢复、恢复后的外部变动冲突，以及长命令运行中继续编辑保存另有人工验收。通过范围、截图及未覆盖项见 [第三阶段测试报告](docs/TEST_REPORT.md)；历史报告保存在 [第二阶段](docs/TEST_REPORT_PHASE2.md) 和 [第一阶段](docs/TEST_REPORT_PHASE1.md)。

## 工程结构

| crate | 职责 |
|---|---|
| cedar-app | 原生界面、编辑状态、异步工作队列、LSP / Recovery / Commands UI |
| cedar-client | 本地与 SSH/子进程传输，握手、超时和断开状态 |
| cedar-protocol | 有界、版本化的工作区 JSON 消息 |
| cedar-workspace | 文件边界、保存、搜索、可信工具、语言服务和异步任务代理 |
| cedar-agent | 远程 stdio 入口 |
| cedar-recovery | 前端本地草稿存储、锁、完整性校验、原子替换和配额 |
| cedar-tasks | 有界异步命令监督、输出捕获、取消和普通子进程组清理 |
| cedar-language | LSP 客户端/生命周期/有界 JSON-RPC；DAP 基础封包 |
| cedar-debugger | 有界异步 DAP 进程传输，尚未接入 UI 或远程 agent |

更多：[功能矩阵](docs/FEATURE_MATRIX.md)、[架构](docs/ARCHITECTURE.md)、[语言服务](docs/LANGUAGE_SERVICES.md)、[Kotlin 验证](docs/KOTLIN_VALIDATION.md)、[调试](docs/DEBUGGING.md)、[前端恢复与命令交互](crates/app/RECOVERY_AND_COMMANDS.md)。下一阶段优先考虑有预览、版本校验和撤销保障的格式化，以及引用查找、文档大纲；多文件重命名暂缓，先解决无版本跨文件结果与文件资源操作遗漏的安全问题。完整重构、项目模型和大量 IDEA 功能仍待实现。

## 打包已验证检查点

`scripts/package_checkpoint.py` 可将干净、HEAD 有精确 tag 的源码与已构建的 Linux `cedar` / `cedar-agent` 打成 ZIP，并记录 source commit/tag、二进制 SHA-256 和平台限制。先完成验证、release 构建以及源码/证据提交与 tag，再运行：

```sh
python3 scripts/package_checkpoint.py --output /absolute/path/cedar-checkpoint.zip
```

它不会构建、签名或发布，也不证明源码与预先构建二进制的可复现对应；二进制为未签名 Linux 检查点，不含 Windows exe、JDK 或语言/调试服务器。

## 许可

本项目代码按 MIT 或 Apache-2.0 双许可提供。依赖保留各自许可；参见 `THIRD_PARTY_NOTICES.md`。没有打包 JDK、JDT LS、Kotlin 服务器或 JetBrains 组件。源码包不含下载缓存、临时凭据、用户项目或大型构建目录。

Public source history and omitted machine-specific evidence are described in [PUBLICATION.md](PUBLICATION.md).
