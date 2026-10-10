# Linux 桌面开发包（0.41）

本包把正常默认特性的 `cedar` 原生桌面前端和匹配的 `cedar-agent` 放在同一个目录中，面向 **Ubuntu 24.04 amd64 / x86_64（glibc）**。它是未签名的开发包，不是安装器、系统服务、通用 Linux 静态程序或完整 Java 开发环境。独立的 Linux agent-only 归档仍是另一个产品包，不能替代本包的桌面前端。

## 环境与验证范围

运行前需要 Ubuntu 24.04 amd64 上兼容的动态加载器、glibc、libgcc 和 libm。两个程序的实际 ELF 解释器、`DT_NEEDED` 与符号版本需求分别记录在 [包清单](BUNDLE_MANIFEST.json) 的 `abi.cedar`、`abi.cedar-agent` 中；最低 GLIBC 需求以这些测量值为准，不能据此扩大为任意发行版、旧版 glibc、Alpine/musl 或 ARM64 支持。

桌面还需要可用的 **X11 或 Wayland 会话**、OpenGL/GLX 或 EGL 运行时及驱动，以及所选后端依赖的 X11/Wayland、xkbcommon 等系统库。部分库由窗口或图形后端在运行时动态加载，不一定出现在 ELF 的 `DT_NEEDED` 中。**ELF 检查通过不证明显示服务器、动态加载库、驱动或图形上下文可用，也不证明 X11 与 Wayland 都通过了原生验收。** 实际测试的显示后端及其结果必须以当前精确提交的原生验收记录为准；本说明编写时该项仍待原生验证，不把 headless 协议测试当作 GUI 验收。

基本文本编辑不需要 Rust、Python、JDK、JDT 或 Maven。包不会下载或安装这些工具、系统库、显卡驱动或字体，也不会修改系统配置。不要复制其他机器的 glibc 或随意替换系统加载器来绕过兼容性问题。

## 文件、来源与解压

包只包含：

- 同次构建配对的 `cedar` 和 `cedar-agent`，两者执行权限均为 `0755`
- 本说明、[Maven 叶工程说明](MAVEN_PROJECTS.md)、[依赖快照说明](MAVEN_DEPENDENCIES.md) 与逐文件大小、权限、SHA-256 清单
- [MIT 许可](LICENSE-MIT)、[Apache 许可](LICENSE-APACHE)、[第三方说明](THIRD_PARTY_NOTICES.md) 和 `third-party-licenses/`

不携带测试探针、验收程序、JDK/JDT、Maven 缓存、额外字体文件、用户项目或凭据。第三方说明可能涵盖工程中未链接到某个程序的组件，不是运行时系统库清单。

1. 从该源提交的项目 CI 获取完整桌面 tar.gz 和对应 SHA-256，核对源提交及 CI 运行链接，保留原始归档与清单。两个程序必须来自同一份完整归档，不要与独立 agent 包或其他版本拼装。
2. 在自己控制的目录中解压，使用一个**尚不存在的新目录**，例如 `Cedar 桌面 测试`。不得覆盖已有程序或用户配置；不要以管理员/root 身份启动本开发包。
3. 可在对应已核对的源码 checkout 中使用 `python3 scripts/package_linux_desktop_bundle.py verify <归档路径> --source-commit <完整提交SHA> --ci-run-url <该次CI链接> --source-root <干净源码目录> --extract-to <新目录>`，进行有界结构、内容与来源文档核验并提取。验证脚本不在桌面包内；Python 仅用于这一步验证。
4. 在图形桌面终端进入解压目录，运行 `./cedar`。保留同目录的 `cedar-agent` 及其执行权限；不要只复制前端。

验证器拒绝不支持的 ELF、错哈希、越界路径、链接、特殊文件、错误权限、多余文件和非规范归档；提取后再检查精确文件清单与内容。哈希和自述源/CI 链接用于完整性核对，**不是发布者签名、可信构建证明或可复现构建证明**。对应 CI 负责从同一提交构建正常默认特性的两个程序；配对握手另由运行验收检查。

## 第一次打开 Local folder

1. 在 **Open workspace → Local folder** 中选择已有工作区的绝对路径。空格和中文路径可用；建议先用自己创建的测试目录。
2. 保持 **Trust this workspace for Git, language servers, and commands** 关闭，然后连接。文件浏览、读取、编辑、保存和文本搜索不需要启用执行信任。保存会写入所选工作区，信任关闭不是只读模式。
3. Linux Local 固定启动**当前前端旁边**的 `cedar-agent`，通过标准输入/输出连接，不搜索 PATH、不接受另选的 Local agent，也不会回退到内嵌工作区。缺失、不可执行或握手不匹配的 agent 会明确报错。应重新获取整份匹配开发包，再显式连接；不要用其他版本替换其中一个文件。
4. 前端会核对本地配对的协议、产品版本与 Linux 平台，并按 agent 握手能力启用功能；能力声明本身不授予执行权限。执行命令、Git 和语言服务都仍须用户显式信任工作区。

本地进程隔离与进程组清理不是安全沙箱。启用信任后，工具可能以当前账户权限运行项目代码、访问文件或网络。不要为不可信项目开启信任，也不要把“离线依赖解析”理解为网络隔离。

## 草稿、恢复与断开

草稿恢复默认开启，在前端电脑保存本地明文副本，可能包含源代码或其他私密内容。Linux 默认使用绝对 `$XDG_DATA_HOME/cedar/recovery`，否则为 `$HOME/.local/share/cedar/recovery`；已有绝对 `CEDAR_RECOVERY_DIR` 设置优先。界面的 Recovery 状态可用于检查副本和设置。默认恢复写入不是自动保存工作区文件，锁冲突、存储错误或配额问题会显示警告，不能把警告状态当作已可靠备份。

恢复需要显式操作，保留原始保存版本，不能用恢复覆盖外部已经修改的文件。恢复、重新连接和读取任务配置都不会自动开启信任或执行工具。需要保留草稿时先保存或复制，不要把关闭/丢弃确认当成备份保证。

停止活动语言服务、取消任务并等到终态后，可以显式 Disconnect。断开保留前端草稿；保存结果未知时不会自动重写。`Disconnecting` 等待所有者清理回执，`Cleanup unverified` 不能当作成功清理。语言会话 Stop 的清理未验证会阻止该 agent 工作区内重启语言服务；连接清理未验证则保留警告，显式重连不会证明旧进程已退出。

## 可选的类型化 Java 与 Maven

只有在确认工作区和工具发行版可信后，才启用执行信任。在 Language 面板选择 **Java / JDT LS**，显式填写本机现有的兼容 JDK 中实际 `bin/java` 绝对路径、包含 `plugins` 与 `config_linux` 的 JDT 发行目录，以及工作区外已有的专用数据目录。Linux 的 Java 路径须为普通可执行文件；不要填 PATH 名称、shell 包装器或 `/usr/bin/java` 这样的符号链接。普通 Java 配置支持空格和 Unicode 路径。

只有显式 **Start server** 才启动服务；打开工作区、解压和恢复都不自动启动。JVM 最大堆为 512 MiB，总进程树内存可能明显更高。数据与 JDT 配置可能写入缓存。

默认 Java 模式关闭 Maven/Gradle 自动导入。需要受支持的单根 Maven JAR 叶工程时，另外选择 **Import root Maven pom.xml (isolated agent, trusted leaf project)** 并填写已有本地 Maven 缓存；data/control 父目录仍要求工作区外的 ASCII 绝对路径。缺少依赖时报告未解析，Cedar 不下载、构建或自动重导入。更多 POM 子集、缓存写入与退出限制见 [Maven 范围](MAVEN_PROJECTS.md)。

Maven 的离线依赖解析**不是进程、文件系统或网络沙箱**：JDT、Buildship 及受信任插件仍可能自行联网或执行代码。不要把关闭自动导入或本地缓存当作安全隔离。完整 Maven/Gradle、交互式 PTY 与 IDE 调试桥接不在本包范围。

## 中文显示与故障判断

中文内容按 Unicode 保留。出现需要的 CJK 文本时，前端会按需查找已安装且受支持的系统字体作为缺字回退，包括固定位置的 Noto CJK / 文泉驿候选；它不会扫描任意字体目录、下载字体或向包中复制系统字体。单个候选文件限 32 MiB。缺少合适字体时可能显示方框，并给出字体状态提示，文字本身不会因此被替换。

启动失败时先核对系统架构、归档完整性、两个程序的权限和同目录配对，再检查当前桌面会话及图形运行库；不要通过关闭验证或扩大加载路径掩盖不匹配。没有图形会话的服务器只能验证 agent/协议部分，不能证明桌面窗口可运行。

本阶段包装和本地配对测试不代表真实 SSH 身份认证、网络断线恢复、远程进程清理、用户电脑或用户项目验收。任何测试探针只属于独立验收树，不随本开发包发货。

## 保存多个已打开文件

Save 旁的下拉菜单提供 **Save all editor buffers**（Ctrl/Cmd+Shift+S）和 **Cancel remaining saves**。按捕获顺序逐个保存，遇到冲突或未知结果停止后续文件，已经成功的保存不会回滚。继续输入保留为新草稿，Run 配置表单须单独 Save profile；详见 [SAVE_ALL.md](SAVE_ALL.md)。
