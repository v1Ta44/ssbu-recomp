# 上游同步与维护者反馈记录（2026-10-16）

## 本次处理范围

- 上游：Denkishi/ssbu-recomp；本地 origin 和 denkishi 均指向该项目。
- 个人 fork：v1Ta44/ssbu-recomp，仅同步 main 和清理明确已采纳的 PR 分支。
- 本地 main、fork/main 已从 3a969166137894b7166039d2a8a3db15cf383877 快进至 938e66bb7be0ba4123e291211895412da421b705。
- 自用 yuzu 的整合基线是 9d099e5a2ebe0539c106cb42c4124792e8da09be；整合分支为 codex/yuzu-upstream-sync-20261016，保留 yuzu 原有全部提交历史，再合入上游 main。
- 当前 D:/pcsmash 的 yuzu 工作区有并行 agent 的未提交修改，因此本次没有切换、暂存、提交、清理或覆盖该工作区，也没有移动 yuzu 分支指针。
- 整合分支仅保存在本地，没有推送自用网络扩展。当前并行修改不包含在整合副本中，后续并入时仍须保留这些修改。
- 本次不执行构建、链接、打包、依赖安装、游戏启动、运行时测试或性能测试。

## 分支处置

维护者先在自己的工作树移植改动，再统一导出；因此这些 PR 在 GitHub 上显示 CLOSED 而非 MERGED。删除依据是其明确的采纳回复，以及上游提交中的逐项说明，不是单看关闭状态或提交祖先关系。

### 已采纳，删除本地与 fork 对应分支

| PR | 功能分支 | 处置依据 |
| --- | --- | --- |
| [#22](https://github.com/Denkishi/ssbu-recomp/pull/22) | pr/fonts-localized-name | 本地化系统字体名称；明确收入 938e66b |
| [#23](https://github.com/Denkishi/ssbu-recomp/pull/23) | pr/startup-romfs-check | 直接启动时查找和校验 RomFS；明确收入 938e66b |
| [#27](https://github.com/Denkishi/ssbu-recomp/pull/27) | pr/build-deps | .inc 和 nvn_vk_*.c 依赖；明确收入 938e66b |
| [#28](https://github.com/Denkishi/ssbu-recomp/pull/28) | pr/diagnostics | 崩溃报告、转储和调试符号；明确收入 938e66b |
| [#31](https://github.com/Denkishi/ssbu-recomp/pull/31) | pr/vk-readback-overrun | 纹理复制越界修复；明确收入 938e66b |
| [#32](https://github.com/Denkishi/ssbu-recomp/pull/32) | pr/vk-device-lost-report | Vulkan 设备丢失诊断；明确收入 938e66b |
| [#33](https://github.com/Denkishi/ssbu-recomp/pull/33) | pr/vk-prewarm-outside-lock | 历史记录与 SPIR-V 锁外读取；明确收入 938e66b |

统一回复的意思是：改动按审阅要求移植到作者工作树，再作为 938e66b 的一部分导出。原始 PR 提交和分支名已保存到可验证的 Git bundle，删除可以恢复。

采纳提交：[938e66b](https://github.com/Denkishi/ssbu-recomp/commit/938e66bb7be0ba4123e291211895412da421b705)。

### 保留的功能分支

| PR | 功能分支 | 状态和保留原因 |
| --- | --- | --- |
| [#24](https://github.com/Denkishi/ssbu-recomp/pull/24) | pr/window-aspect-f11 | 作者已审阅，答复将在下一次更新移植；仍开放，当前上游尚未包含 |
| [#25](https://github.com/Denkishi/ssbu-recomp/pull/25) | pr/audio-wav-from | 用户自行关闭，无作者采纳回复 |
| [#26](https://github.com/Denkishi/ssbu-recomp/pull/26) | pr/run-exclude-cpus | 用户自行关闭，无作者采纳回复 |
| [#29](https://github.com/Denkishi/ssbu-recomp/pull/29) | pr/vk-memory-modules-libraries | 仅 container_seen 崩溃修复被采纳；模块释放和阶段库清理仍未采纳 |
| [#30](https://github.com/Denkishi/ssbu-recomp/pull/30) | pr/gl-texture-range-index | 作者已审阅，答复将在下一次更新移植；仍开放，当前上游尚未包含 |
| [#34](https://github.com/Denkishi/ssbu-recomp/pull/34) | pr/vk-announced-queue | 核心思路获认可；等待作者重做着色器准备流程后再更新和测量 |

另外保留 main、yuzu、backup/port-2026-10-16、backup/yuzu-wip-2026-10-16；增加本地整合分支，不改变其他分支内容。

## 作者反馈对应的后续事项

### #29：模块释放与阶段库清理

- 已采纳的 container_seen 修复采用上游实现：在锁外读取可能被游戏释放的内存，回锁后确认条目未被其他线程改变，再失效对应记录。
- 若未来继续贡献，将模块释放和阶段库清理拆成两个 PR。
- 模块释放：SPIR-V 重新读取失败应有非致命处理。
- 阶段库清理：实验功能应默认关闭。
- 本次同步保留自用 yuzu 中尚未采纳的模块释放与清理功能，没有把上述未来投稿要求作为本次功能改造。

### #34：预告容器独立队列

- 等待作者发布新的着色器准备流程后再 rebase；他的目标是在最初白屏完成准备，战前加载不再承担着色器工作。
- 将 g_light_run 连续 120 帧的判断规则独立拆出；核查暂停、过场、电影是否可能触发战斗中的误判。
- 非加载屏幕等待期间，预告任务改用 BELOW_NORMAL 优先级。
- _stalls.txt 输出补齐预告队列；现有 watchdog 报告中的队列输出予以保留，作者提到的另一处日志要求仍需核实。
- 独立测量此改动，排除 #29 的干扰；正常游戏速度，A/B 每侧至少三次。
- 分别覆盖首次准备已完成（默认场景）与未完成的情况。
- 报告战斗中的等待次数，以及战后加载屏幕被额外延长的时间。此前的突发任务数据来自只准备了 132/521 个容器的状态，不能直接代表默认场景。
- 本次保留自用版本的现有队列和阈值；不启动性能测量。

### [Issue #36](https://github.com/Denkishi/ssbu-recomp/issues/36)：房间协议与本地无线

用户说明 yuzu 属于自用扩展，暂不准备提交；以下作为维护者意见记录，供以后决定是否投稿时参考。

- 当前维护者暂不接收进 main；先保留在 fork，待两台电脑跨客户端完成整局再讨论。
- 其回滚方案正在推进状态保存和恢复，并测试提前运行一帧；本地无线的锁步同步会发送真实网络包，不能随着推测帧一起回滚，因此两种模式应互斥。
- 若要投稿，网络支持应为编译选项，默认程序不含网络代码和 ENet/Zstd；编入但关闭时，确定性模式的哈希日志应与未编入版本一致。
- 不允许链接步骤下载依赖。可随仓库附带源码和许可证，或增加可选 setup.py 安装步骤；采用官方发布地址、SHA-256，并支持将归档手动放进 deps/。
- 许可证兼容方向可接受；各头文件补 SPDX 和参考的 yuzu 源文件说明，并保留来源说明文件。
- 昵称必须明确输入，不得直接发送可能是真实姓名的 Windows 用户名；日志不记录房间密码、服务器地址或成员地址。
- 对房间数据的长度、数量、解压输出严格设限，对解析器进行模糊测试；只在游戏自己的 recv/LDN 调用中写入游戏内存。
- nifm 继续报告无互联网，避免在线模式访问 Nintendo 服务。
- 使用兼容 yuzu 系房间的中性命名；用户手填主机、仅密码房间，不附带默认服务器、大厅列表或网页令牌流程；独立自托管房间工具可考虑随附。
- 对游戏版本、角色源码、模组、新角色种类等不一致进行拒绝或提示；移植版之间可交换构建指纹。
- 若以后投稿，先完成两台 PC 的整局，再尽可能验证移植版与 yuzu 互通，并分别测试双方作为房主。保留双方帧日志、RTT 下的停顿和新增延迟数据。
- 投稿拆分顺序：房间客户端、socket 和测试；LDN；控制窗口。BUILDING.md 补每个开关，审计条目标明哪些行为参考了 yuzu LDN 而非主机实测。

## 本次整合的处理与验证

- 解决 10 个冲突文件，已采纳部分使用维护者整理后的实现；保留网络启动/退出挂钩、网络链接接入、画面比例和 F11、音频录制开关、CPU 排除、纹理范围索引、模块释放/库清理及预告队列。
- 接受上游的电影、Vorbis、DLC、角色层和构建依赖更新。
- 检查并去掉自动合并产生的重复 CopyLayout/copy_layout、debug_link_flags 及过时注释。
- 对上游未修改的 39 个本地扩展文件核对，与 yuzu 基线完全一致；网络实现和打包工具包含在该核对范围内。
- 12 个 Python 文件通过纯文本 AST 语法解析；没有执行这些脚本，也没有导入项目构建模块。
- 检查合并标记、空白错误和新增重复 C 函数定义；均未发现遗留问题。
- 未进行 C 编译或运行验证，因此本记录不宣称合并结果已经通过构建或游戏测试。

## 备份与继续接入

- 备份与审计目录：D:/pcsmash-sync-records/2026-10-16/。
- Git bundle：adopted-feature-branches.bundle，包含原 main、原 yuzu 和删除的七个分支，已完成 bundle verify。
- 同目录保留原始分支快照、初始工作区状态、作者回复、删除清单、冲突处置及静态检查结果。
- 独立整合目录：D:/pcsmash-sync-20261016/。
- 当前工作区的并行任务结束、修改妥善提交后，再将 codex/yuzu-upstream-sync-20261016 合入 yuzu；若 yuzu 后续又有提交，使用常规合并并保留后续增量，不做重置覆盖。
- 不要将该整合分支推送为上游 PR；其中包含用户仅供自用的扩展。

## 接续：并行任务完成后的合并、构建与打包（2026-10-16）

以下记录覆盖前文“尚未应用到当前 yuzu”的历史状态；首次隔离同步阶段的记录保留用于追溯。

### 合并和增量保护

- 将并行任务留下的 35 个文件变更先提交为 2fde76e83be997bdee3e49d22e6231043b342aa3，包括控制台、管道桥接、LDN 取消与清理修复、接口文档和打包接入。
- 创建 backup/yuzu-before-merge-20261016，并另存 D:/pcsmash-sync-records/2026-10-16/yuzu-before-merge.bundle。
- 在 yuzu 上合入 codex/yuzu-upstream-sync-20261016，得到合并提交 b2212a752ffc1fafc891f2904952757b39dcb948。
- 两个父提交分别为 2fde76e 和 742fd83；上游 938e66b 与并行任务的新增内容都在当前分支历史中。
- 本轮合并自动完成，无新增冲突；首次整合阶段解决的 10 个文件冲突沿用已核对结果。
- 核对控制台源码、网络桥接、LDN、打包和接口文档未被上游覆盖，boot.c 的控制台入口及构建源文件表中的 console_bridge.c 均保留。
- 仅本地合并自用 yuzu，没有将自用扩展推送到远程。

### 构建和链接

- 使用项目 .venv 的 Python 3.11.9 和既有 LLVM 9.0.0 工具链。
- 执行 scripts/setup.py --dump D:/pcsmash/work/dump --from compile --jobs 16：完成源码同步、主游戏库和 98 个脚本模块的增量构建。
- 首次链接遇到旧游戏进程占用 ssbu_native.exe；正常关闭开发目录内的控制台和游戏后，使用 --from link 完成链接。
- 最新运行时、网络、控制台桥接已链接到 build/work/ssbu_native.exe，PDB 和 MAP 同步生成。
- 控制台执行 cargo build --release --locked --offline，使用 build/console-target，发布版构建成功。
- SDK 自检发现一项历史失败，因此 setup.py 的整套 check 步骤未标记通过；后续检查单独执行并分别记录结果。

### 验证结果与限制

| 检查 | 结果 |
| --- | --- |
| 控制台 Rust 单元测试 | 6 通过，0 失败 |
| 手柄自检 | 49/49 通过 |
| 网络后端回环夹具 | 房间协议、LDN 两种角色、压缩数据报、畸形包、断开唤醒和日志脱敏均通过 |
| 诊断启动测试 | 2 通过，0 失败 |
| 控制台桥接实测 | poll、日志等级校验、昵称校验、连接超时、leave、关闭管道后进程正常退出均通过 |
| SDK 自检 | 1 项失败：erfcf 与宿主 CRT 比较最大差异 5 ULP；2026-10-14 的旧日志有相同失败 |
| 指令差分测试 | 生成 32,043 个测试并编译测试程序；Unicorn 2.1.4 在 mem_map 阶段发生访问异常，进程退出 -1073740791，尚未取得指令比较结果 |
| 包完整性 | 142 个带哈希记录的文件 SHA-256 一致；ZIP 全部条目 CRC 检查通过；保留 13 个 .nrr 文件 |

没有修改数学函数来掩盖 SDK 自检差异，也没有调整系统安全设置或替换 Unicorn 依赖。未进行双 PC 互通、完整对战或跨 GPU 测试。

### 本次产物

- 构建来源：yuzu 的 b2212a752ffc1fafc891f2904952757b39dcb948，打包时工作区干净。
- Build ID：eefc23c34554。
- 完整目录：D:/pcsmash/build/dist/ssbu-yuzu-20261016-b2212a7/。
- 启动入口：上述目录中的 SSBU Console.exe。
- ZIP：D:/pcsmash/build/dist/ssbu-yuzu-20261016-b2212a7.zip，422,860,660 字节，约 403 MiB。
- 完整目录通过硬链接引用本机已有的 data.arc；ZIP 不包含 data.arc，使用时需要已有 RomFS。
- 打包使用 --romfs auto --cache containers --config --zip；未使用 --allow-stale，没有覆盖既有安装包。
- 符号库：D:/pcsmash/build/dist/symstore/，包含本次 EXE 和对应 PDB。
- 完整性结果：D:/pcsmash-sync-records/2026-10-16/package-verification.json。

### 日志与记录位置

仓库内记录：D:/pcsmash/docs/UPSTREAM_SYNC_2026-10-16.md。

同内容的独立记录：D:/pcsmash-sync-records/2026-10-16/UPSTREAM_SYNC_2026-10-16.md。

构建和测试日志位于 build/logs/yuzu-*-20261016.log；本次日志也复制到 D:/pcsmash-sync-records/2026-10-16/rebuild-logs/，SDK 自检原始日志单独保留。
