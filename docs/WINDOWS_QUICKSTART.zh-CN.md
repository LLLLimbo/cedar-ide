# Cedar Windows 开发包

这是 Windows x86-64 的未签名开发版本，可解压运行。它不是安装器，也不是
IntelliJ IDEA 的完整替代品。请从本项目对应提交的 GitHub Actions 下载，保留
同目录的 `cedar.exe` 与 `cedar-agent.exe`；不要混用不同版本。

## 第一次打开

1. 将整个 ZIP 解压到一个你有写入权限的新目录。路径可以包含空格和中文。
2. 打开 `cedar.exe`，选择 **Local folder**，填写已有项目目录的绝对路径，
   点击 **Connect workspace**。初次试用可先选择一份测试文件的副本。
3. 保持 **Trust this workspace for Git, language servers, and commands** 关闭。
   浏览、编辑、搜索和保存文件不需要执行信任。点击目录中的文本文件打开，
   Ctrl+S 保存，Ctrl+Z 撤销，Ctrl+P 打开路径，Ctrl+F 在文件内查找。
4. 文件在外部发生变化时，保存会拒绝覆盖。用 **Compare with disk** 检查差异。
   连接中断后若保存结果不明，先显式核对磁盘状态，保留当前草稿，不要假定已保存。

程序需要可用的 Windows 图形会话及 OpenGL 驱动。若系统阻止未签名程序，
不要关闭安全保护；可保留错误信息，或自行审查并从源码构建。

## 文件与恢复

“解压运行”只描述程序分发，不代表所有用户数据都保存在解压目录。
默认草稿恢复副本位于 `%LOCALAPPDATA%\Cedar\recovery`，是本地明文，可能包含
源码中的秘密；Windows 继承目录 ACL。Recovery 面板可查看位置、状态及副本。
最近一次确认备份之后的输入仍可能丢失。关闭会话备份不会删除已有副本。
程序不会自动把解压目录当成你的项目，也不会自动运行项目命令。

## 可选 Java 支持

普通文本编辑不需要 Java。Java 语言服务需要你另行安装 JDK 21+ 和 Eclipse
JDT LS；本包不包含或自动下载它们。已验证的 JDT LS 版本为 1.61.0。
完整配置与限制见同目录 [WINDOWS_JAVA_SETUP.md](WINDOWS_JAVA_SETUP.md)。

Language 面板选择 **Java / JDT LS**，配置现有原生 `java.exe` 的 ASCII 绝对路径、
JDT 安装目录，以及工作区之外的独立数据目录。项目、JDT 和数据目录可含中文；
UNC、设备路径和任意 JVM 参数不在当前支持范围。启动语言服务或命令需要你明确
信任该工作区；只编辑配置不会启用信任，也不会运行程序。

Java 最大堆为 512 MiB，但 JVM 总内存可以明显高于这个数字。此前同一合成项目的
无界面测试驱动、发行版 agent 与 JVM 合计工作集中位数约 819–840 MiB；这不是
完整 GUI 占用，也不是与 IDEA 的等条件比较。GC 诊断观察到末次 GC 后堆占用
378 MiB、容量 512 MiB，这不是最终空闲时的存活对象量。当前没有降低堆或更改 GC。

Stop 会区分自然退出与超过宽限后的强制清理；**forced / grace_expired** 不是
正常退出。清理未验证时界面会阻止重启，须先检查错误并重新连接。
Maven/Gradle 导入、JDK class-file 查看、Windows Git、同步 Run、通用 Windows
LSP 和完整调试界面仍未提供。异步命令使用明确的原生 `.exe` 绝对路径。

## 验证与来源

`BUNDLE_MANIFEST.json` 记录版本、精确源码提交、CI 运行链接，以及每个有效载荷
文件的 SHA256 和大小。外部 ZIP SHA256 随 CI 构建回执提供。哈希用于检查文件
是否一致，不代替代码签名；不要把同一下载内的哈希视为独立可信来源。

原生 Windows CI 会将包解压到含中文和空格的路径，用单独的非发布测试程序经
正常 Client/agent 路径验证信任关闭时的浏览、读取、修改、保存、冲突拒绝和清理。
测试程序不随包分发。这是无界面验证；当前版本尚未完成 Windows 原生 GUI 交互
和真实认证 SSH 的独立验收。SSH 能力仍属于开发中的独立验证范围。

许可证及第三方声明随包提供。问题反馈请附版本、清单中的提交和错误信息，
不要公开源码秘密、私钥、令牌或原始 JVM 日志。
