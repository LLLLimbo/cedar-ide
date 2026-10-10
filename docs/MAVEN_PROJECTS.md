# Windows / Linux 隔离 agent Maven 叶工程模式

0.22 增加 Windows 隔离 agent 专用 Java 路径的显式 Maven 选项；0.39.1 将相同子集扩展到 Linux 独立 agent，已通过精确 Ubuntu 原生验收。它读取工作区根目录的
`pom.xml`，通过已安装 JDT LS/m2e 建立源码、编译器设置和依赖模型。
这是有意收窄的叶工程支持，不是完整 Maven、Gradle 或构建管理器。
对应提交的原生 CI 与开发包清单才是该二进制的验证依据；跨平台编译不等于 Windows 运行验证。

## 使用

1. 选择可信的单根工作区，其根目录已有符合下列子集的 `pom.xml`。
   此功能要求现有的执行信任；能力声明不会自动开启信任。
2. 在 **Java / JDT LS** 中填写工作区主机上的 Java 可执行文件、JDT distribution、
   JDT data directory。Java 与 JDT 必须已安装，Cedar 不安装它们。
3. 选择 **Import root Maven pom.xml (isolated agent, trusted leaf project)**，填写
   **Local Maven repository**。这是已有的本地缓存目录，不是远程仓库 URL。
4. 显式 **Start server**。启动可取消，沿用原有的启动 ID、期限和所有者清理。
   初始化完成不表示项目已经完成所有索引工作。
5. 显式 **Check Maven model** 获取一次只读快照。界面显示源码路径、编译器设置、
   类路径和未解析依赖。没有后台模型轮询或自动重导入。

启动进程须具有干净的 Java/Maven 启动环境。除普通 Java 模式已拒绝的变量外，
若继承了 `MAVEN_OPTS`、`MAVEN_ARGS`、`MAVEN_CONFIG`、`MAVEN_USER_HOME`、
`M2_HOME`、`MAVEN_HOME`、`MAVEN_PROJECTBASEDIR`、`MAVEN_CMD_LINE_ARGS` 或
`MAVEN_EXT_CLASS_PATH`，本模式会拒绝启动，即使变量值为空。
这是当前保守兼容边界；Cedar 不修改机器或父进程的环境配置。

普通 Java 模式仍关闭 Maven/Gradle 导入。旧 agent、通用 LSP 与不支持此配置的 agent
不会隐式使用这个新模式；缺少完整 Maven 能力时必须明确选择普通 Java。

## 路径与缓存

- data/control 父目录必须是已有、位于项目外的 **ASCII 绝对路径**。这是初期的
  JDK 启动器 `user.home` 参数限制。工作区、JDT distribution 和 Maven cache
  可以使用 Unicode 路径。Windows Java 可执行文件仍要求 ASCII 本地盘绝对路径；
  Linux 则使用原生绝对 java 路径，可以包含 Unicode，仍须是普通可执行文件。
- 每次启动都在选定的 data 目录下新建一个 Cedar 子目录，放置干净的用户/全局
  Maven settings、隔离的 home/tmp 和新的 JDT 数据。不会覆盖用户原有配置。
- 这些新建的数据和索引目录会保留，因此重新启动会产生额外磁盘占用并重新建立索引。
  如需清理，必须先确认服务及 agent 已停止；本模式不会自动删除用户选定的目录。
- 缓存不仅需要项目依赖，还需要当前 JDT/m2e 使用的 Maven 插件及其依赖。
  不完整的缓存会报告未解析状态或启动/模型错误。Cedar 不替用户下载缺失内容。
  CI 中的固定缓存准备工具只服务于合成测试，不是产品的依赖安装器。

## 支持的 POM 子集

当前允许根元素中的 `modelVersion`、`groupId`、`artifactId`、`version`、`packaging`、
`properties`、`dependencies` 和 `build`。包装类型只能是 JAR；所有配置值须为有限的
字面量，不支持 `${...}` 插值。属性限编译器 source/target/release 与源码/报告编码。
直接依赖限 JAR、普通 compile/provided/runtime/test scope，可指定字面量 classifier。
build 限项目内的 source/test-source/output/test-output 相对目录。

父 POM、reactor/modules、profiles、项目插件/扩展、BOM/dependencyManagement、
自定义 repositories、systemPath、wrapper、现有 `.mvn`/Eclipse 工程配置、外部构建路径、
DTD/实体/CDATA 等均不在本阶段支持范围内。额外根元素也会明确拒绝，而非静默忽略。
POM 最大 128 KiB，最多 256 个直接依赖；XML 深度/事件数及返回模型也有界。
这是显式兼容性子集，不是通用 Maven XML 解释器。

## 磁盘模型、草稿与错误

模型绑定当前语言会话和启动时读取的 **磁盘 POM SHA-256**。未保存的 POM 草稿不会
自动写盘，也不会成为导入模型。已有旧草稿及其 Undo 历史会保留。
启动后确认的新 POM 读写版本或后端模型检查发现磁盘 POM 改变时，会要求 Stop 后重新启动；
不会自动保存、重新导入或重放操作。尚未被读取的外部变化由下次显式模型检查发现。

`imported` 表示取得了符合当前边界的模型快照，**不表示编译成功、诊断全部到达、所有索引已稳定，
或验证了某个 JDK 的完整平台 API**。`unresolved` 可以包含一个指向缺失 JAR 的声明；
该声明不是已加载的依赖。未解析依赖与缺失的可选源码目录分开计数。
`unavailable` 不会被当作成功导入。POM 错误可在已有诊断列表中查看。

确认语言服务退出后，当前模型行和待处理模型查询的采用资格会撤销，模型面板明确显示服务已退出，
**Check Maven model** 禁用并提示先显式 Stop、再重新启动。原磁盘 POM 身份与未保存草稿保持不变；
晚到的模型回复或后续 POM 读写回执不会恢复旧模型。已发送的请求仍按原顺序和期限排空，
不会自动重新查询或启动服务。服务退出不等于已验证进程清理；Stop 的结果仍单独核对。

本模式不运行 Maven goal、wrapper、自动构建或 Save All。需要构建时仍由用户另行配置并
显式运行普通命令任务。现有 Java 诊断刷新、预览编辑和 Stop 结果仍保留各自限制；
强制清理不称作自然退出，清理未验证时不允许直接重启。

## “离线”的含义

Maven 设置为离线解析，并使用指向新建空目录的 file-only mirror。所选缓存仍可能产生
解析标记，JDT 会写入索引，受信任的 Maven configurator/plugin 代码可能被加载。
它仍以运行账户的权限执行，**不是代码、文件系统或网络沙箱**。
即使不执行构建 goal，m2e 也可能为检查生命周期映射而尝试离线解析默认插件，
并在缺失插件目录中写入小型解析状态文件。这不代表下载了插件或执行了其 goal。

固定版本 JDT 会在客户端初始化前启动 Buildship；后者可能请求公开的 Gradle 版本元数据，
即使关闭了 Gradle 工程导入。普通 HTTPS/TLS 与重定向行为仍适用。
因此这里只承诺 Maven 依赖离线解析，不能声称整个语言服务器没有网络活动。
参见[固定 JDT 启动路径](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/08eafe6ff60c7159ef88571d47b6a9ef82fef94e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/JavaLanguageServerPlugin.java#L185)
与[固定 Buildship 元数据请求](https://github.com/eclipse-buildship/buildship/blob/75386458f55a1c06c595973831a91fdf4ef7fe20/org.eclipse.buildship.core/src/main/java/org/eclipse/buildship/core/internal/util/gradle/PublishedGradleVersions.java#L156)。

生产 JVM 仍为 512 MiB 最大堆，总进程树工作集可显著高于堆上限。本阶段不声称资源优化、
大型项目兼容、原生 Windows GUI 或真实 SSH 验证已经完成。

Linux 需要支持能力分组的匹配前端（0.38+）和已启用该配置的独立 agent。31 个直接能力名之外，两个有界分组分别声明核心 Maven 操作和可选依赖快照；Windows 保留直接能力名。内嵌 Client Local 不支持此配置；0.41 GUI Local 改用匹配的同目录独立 agent（本阶段待验收），没有自动回退。Linux 使用 config_linux 和原生绝对 java 路径；ASCII 数据/控制目录限制仍保留，工作区、JDT 分发及缓存可以使用 Unicode。Stop 会区分正常退出码和信号；强制清理不是优雅退出，未确认清理会阻止同会话重启。
