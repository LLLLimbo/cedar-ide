# Cedar IDE · Rust 原生远程开发底座

Cedar 是一个可构建、可运行的独立 Rust IDE 工程。目标是降低本地前端负担，并把远程开发作为核心路径。当前是持续开发中的第二阶段工程（0.2.0），**不是 IntelliJ IDEA 的完整替代品**，也不兼容其插件；没有复制 JetBrains 的专有实现或使用其产品标识。

## 已能实际使用

- 原生 egui/Glow 桌面界面，无 WebView、Electron 或前端 JVM
- 本地目录与 SSH 工作区，共用同一文件、搜索、工具和语言服务后端
- 目录浏览、多标签编辑、新建文件、行号、简单 Java/Kotlin/Rust 高亮
- Ctrl/Cmd+P 打开路径、Ctrl/Cmd+F 文件内查找、Ctrl/Cmd+S 保存、Ctrl/Cmd+W 关闭标签
- SHA-256 版本检查；文件在外部改变时拒绝覆盖；原子替换和权限保留
- 脏标签/退出确认；断线保留当前进程中的草稿；显式重连
- 项目文本搜索与跳转；显式信任后读取 Git 状态和执行带超时的命令
- agent 侧真实 LSP：启动、初始化、文件同步、补全/定义/悬浮请求、诊断事件、停止与重启
- 350ms 防抖自动同步、自动诊断列表、F12 定义跳转、类型悬浮、Ctrl+Space 补全
- 补全可延迟解析 import，校验所有编辑后一次性改动草稿；一次撤销/重做覆盖整个操作；不执行服务器返回的任意命令
- 中文注释/字符串可按需加载现有系统 CJK 字体，无静默下载或打包系统字体
- 独立 DAP 子进程传输已经用真实 Python 调试器验证断点/栈/变量；尚未接入 IDE 调试界面或远程代理

## 构建与运行

需要 Rust stable。该版本用 **rustc 1.99.0** 构建和检查，Cargo.lock 固定依赖版本。

```sh
cargo build --release --workspace --locked
cargo run -p cedar-app --bin cedar -- examples/demo
```

Windows 使用 Visual Studio C++ Build Tools / MSVC Rust 工具链；可执行文件会位于 `target\release\cedar.exe`。Linux 需要桌面会话及 OpenGL、X11 或 Wayland 运行时。首次构建需要下载 crates.io 依赖。

本次已做 Linux 编译/测试和实际窗口交互、Windows MSVC 目标的全工作区编译检查。**未在 Windows 运行，也没有交付经过 Windows 运行验证的 exe**。仓库附带 Linux/Windows CI 配置，但它尚未在外部服务运行。

## 远程开发

1. 在远程 Linux 机器构建 `cargo build --release -p cedar-agent --locked`
2. 将 `cedar-agent` 放到自己选择的远程目录；当前不自动上传或安装
3. 在系统终端完成 SSH 登录和服务器指纹核验。前端只使用已有 OpenSSH 配置/凭据
4. 启动 Cedar，选择 **Remote over SSH**，填写 `user@host` 或 SSH 配置别名、端口、远程绝对目录、agent 可执行路径
5. 如需 Git、构建命令或语言服务，只对可信工作区启用工具执行

前端与 agent 必须同版本：0.2.0 使用协议版本 2，会拒绝旧版 agent。

连接通过 `ssh -T` 的标准输入/输出传输有界 JSON，不开启服务端 TCP 监听。开启严格主机密钥检查，禁用 agent/X11/端口转发，不自动信任主机、不生成密钥、不保存密码。

SSH 远程 shell 目前要求 POSIX；Windows 前端连接 Linux 远程工作区是本阶段的主要 Windows 路径。远程 JDK/语言服务器运行在服务器端。当前验证了真实子进程协议链和 SSH 参数/引用规则；没有连接你的服务器，也没有用真实 SSH 服务器完成认证连通性测试。

## 信任与平台边界

- Git status 也可能通过仓库配置触发过滤器，所以与命令、语言服务器一起要求显式信任
- Linux/macOS 命令使用进程组清理；Linux 已实测，macOS 未运行。恶意自行脱离进程组的程序仍不能被当作沙箱内运行
- Windows 本地命令、Git 和语言服务器启动暂时禁用，等待 Job Object 和可取消管道实现。普通本地编辑不受影响，Windows 前端可使用 Linux 远程工具
- 路径访问拒绝绝对路径、越界和符号链接。它不是抵抗恶意并发文件系统修改的 OS 沙箱
- 保存会做晚期版本复核，但无法对不合作的外部写入者提供文件系统级原子 compare-and-swap
- 草稿只保留在内存中；崩溃、强制结束或断电没有恢复保证。重要修改请保存或复制
- 外部语言服务器可能需要 JVM，并会自行索引/执行构建探测；Rust 前端不意味着整个 Java/Kotlin 工具链不使用 JVM
- 官方 Kotlin 服务最新包要求新的 EULA，且已校验的包缺少 EULA 文件，未接受任何协议；已验证的社区替代版本已弃用，不能据此声称支持当前 Kotlin 全特性

## 当前资源策略

按需重绘，无常驻全项目索引；I/O 与工具执行不阻塞 UI；单文件后端上限 1 MiB、编辑标签上限 32、协议帧上限 8 MiB；搜索、进程输出和语言事件有界。大文本降级为无高亮显示，项目代码不发送到云模型或分析服务。

这些是架构措施，不等于“比 IDEA 节省 X%”。目前没有在同一机器、项目与功能集下完成 IDEA 对照基准。详见 `docs/PERFORMANCE.md` 与 `docs/TEST_REPORT.md`。

## 验证

```sh
bash scripts/verify.sh
```

包含格式、严格 Clippy、全工作区测试、独立 agent 协议闭环、agent→LSP 子进程闭环。Windows 可分别执行这些 Cargo 命令，并按 CI 的 PowerShell 示例运行进程测试。

## 工程结构

| crate | 职责 |
|---|---|
| cedar-app | 原生界面、编辑状态、异步工作队列、LSP 面板 |
| cedar-client | 本地与 SSH/子进程传输，握手、超时和断开状态 |
| cedar-protocol | 有界、版本化的工作区 JSON 消息 |
| cedar-workspace | 文件边界、保存、搜索、可信工具、语言服务代理 |
| cedar-agent | 远程 stdio 入口 |
| cedar-language | LSP 客户端/生命周期/有界 JSON-RPC；DAP 基础封包 |
| cedar-debugger | 有界异步 DAP 进程传输、确定性与真实 Python 适配器验证，尚未接入 UI |

更多：`docs/FEATURE_MATRIX.md`、`docs/ARCHITECTURE.md`、`docs/LANGUAGE_SERVICES.md`、`docs/KOTLIN_VALIDATION.md`、`docs/DEBUGGING.md`、`docs/TEST_REPORT.md`。

## 许可

本项目代码按 MIT 或 Apache-2.0 双许可提供。依赖保留各自许可；参见 `THIRD_PARTY_NOTICES.md`。没有打包 JDK、JDT LS、Kotlin 服务器或 JetBrains 组件。源码包不含下载缓存、临时凭据、用户项目或大型构建目录。

Public source history and omitted machine-specific evidence are described in [PUBLICATION.md](PUBLICATION.md).
