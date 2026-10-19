# 个人功能分支整理与 yuzu 整合（2026-10-18）

## 当前状态补记（2026-10-18）

- `D:/pcsmash` 已切换到 `yuzu`；切换时提交为 `45a5696`（修复 Console 对新版模组构建记录的识别）。`main` 与 `origin/main` 仍为 `938e66b`。
- `mod-adapt` 的已提交内容已完整进入 `yuzu`，按用户要求删除了本地分支；原有 6 个已跟踪文件改动和 10 个未跟踪调查/测试文件已完整归档并验证校验值。
- 本机调查归档：`D:/pcsmash-archives/mod-adapt-investigation-20261018-213326/`；同名 ZIP 及 SHA-256 校验文件位于其上级目录。归档包含文件原始内容、补丁、基准提交和恢复说明。
- 原 `yuzu-texture-fix` 工作目录保留在 `45a5696` 的分离状态，以释放 `yuzu` 供 `D:/pcsmash` 使用。构建包、游戏数据和依赖继续保留。
- 用户确认联机扩展已通过跨 Yuzu、PC 客户端之间的双人及三人真实测试；详见 [联机验收记录](YUZU_NETWORKING.md#15-用户确认真实联机测试通过2026-10-18)。

下文为此前整理阶段的历史快照；其中“保留 mod-adapt 分支”“未提交现场原地保留”等描述已由本节更新。

本仓库的固定分工：

- `main` 跟踪 `origin/main`（Denkishi/ssbu-recomp），保持上游原始副本。
- `yuzu` 跟踪 `fork/yuzu`（v1Ta44/ssbu-recomp），容纳个人功能和暂不计划提交上游的修改。
- `mod-adapt`、延迟滑块等个人功能子分支只合入 `yuzu`；已完成功能不代表计划提交上游。
- 向上游提交的内容单独使用明确授权的 PR 分支，不以 `yuzu` 或个人功能集合直接更新上游。

## 本次整合与目标分支纠正

`mod-adapt`、`codex/yuzu-mod-integration` 和 `codex/yuzu-latency-slider` 原本已经是
`yuzu`（`dfb6e74`）的祖先，两项功能无需重复合并。新增的独立构建开关从这个 yuzu 提交继续开发。

先前误将 yuzu 合入本地 main 并推进到 `42d1e1e`。本次纠正将 main 恢复到操作前的
`938e66b`，与已记录的 `origin/main` 完全一致；将构建开关修改直接移到 yuzu 历史上，
不把误合并记录带入 yuzu。原有 Console、联机和个人修复继续只留在个人分支。
所有操作仅在本地进行，没有推送远程或修改上游 PR。

本次没有引入独立帧索引实验，也没有删除已有功能分支或改动 D:/pcsmash 的未提交排查现场。

## 所有本地工作分支

以下提交号为开始整理时的快照；纠正后的分支位置以 Git 实际引用为准。

| 分支 | 快照提交 | 处理与状态 |
|---|---|---|
| `backup/port-2026-10-16` | `bb710e8` | 历史/WIP 备份，仍有独有提交；保留，不自动合并或删除 |
| `backup/yuzu-before-merge-20261016` | `0e7bb2c` | 原有提交已完整纳入 yuzu；保留引用和已有工作目录 |
| `backup/yuzu-wip-2026-10-16` | `6d06d83` | 历史/WIP 备份，仍有独有提交；保留，不自动合并或删除 |
| `codex/main-optional-features` | `42d1e1e` | 先前误合入 main 的本地操作记录，保留作回溯；不作为整合目标 |
| `codex/yuzu-optional-features` | `dfb6e74` 起点 | 本次构建开关修正分支；仅整合到 yuzu |
| `codex/vk-compute-retirement` | `46bbc16` | 独立生命周期修复分支；yuzu 已含后续修复，提交历史/探针仍有差异，保留 |
| `codex/yuzu-frame-index` | `e8b8c0c` | 独立帧索引实验，OneBehind 已撤回；保留实验/上游 FPS++ 调研记录，未并入 |
| `codex/yuzu-latency-slider` | `2d38f34` | 原有提交已完整纳入 yuzu；保留引用和已有工作目录 |
| `codex/yuzu-mod-integration` | `0fea512` | 原有提交已完整纳入 yuzu；保留引用和已有工作目录 |
| `codex/yuzu-upstream-sync-20261016` | `fe99d92` | 原有提交已完整纳入 yuzu；保留引用和已有工作目录 |
| `main` | `938e66b` | 上游原始副本，与 origin/main 一致；不接收个人功能整合 |
| `mod-adapt` | `fc0ce69` | 原有提交已完整纳入 yuzu；保留引用和已有工作目录 |
| `pr/audio-wav-from` | `0447b08` | 上游提案分支；补丁等价内容已纳入整合，保留 PR 历史 |
| `pr/gl-texture-range-index` | `02ed888` | 上游提案分支；补丁等价内容已纳入整合，保留 PR 历史 |
| `pr/oe-focus-wakeup` | `fe87ec0` | 上游提案分支；补丁等价内容已纳入整合，保留 PR 历史 |
| `pr/run-exclude-cpus` | `e71aab2` | 上游提案分支；补丁等价内容已纳入整合，保留 PR 历史 |
| `pr/vk-announced-queue` | `40fc27b` | 上游提案分支；整合含相关功能，但提交不完全等价，保留 PR 历史 |
| `pr/vk-memory-modules-libraries` | `72091fa` | 上游提案分支；补丁等价内容已纳入整合，保留 PR 历史 |
| `pr/window-aspect-f11` | `427fc6b` | 上游提案分支；补丁等价内容已纳入整合，保留 PR 历史 |
| `yuzu` | `dfb6e74` 起点 | 个人修改版本；包含两项已完成功能，并接收独立构建开关 |

## mod-adapt 目录中的未提交内容

`D:/pcsmash` 继续停留在 `mod-adapt`，其未提交代码、未跟踪测试和调查文档原地保留。
4 个 NVN 文件主要是缓冲区/内存池释放前结束异步计算回写；逻辑已有已提交版本纳入 yuzu。
两个内存退役测试也已有 yuzu 提交版本。两个资源测试脚本虽被状态列表标为修改，但规范化 Git 哈希与 HEAD 相同，没有实际内容差异。另有 8 份崩溃、闪黑调查记录。
地图闪黑仍未确认根因或修复，不把这些未提交调查内容混入已完成特性的合并。

## 构建与验证

两项默认关闭，分别使用 `--with-mods` / `--without-mods`、
`--with-latency-slider` / `--without-latency-slider`。正常 setup 构建、直接宿主链接和打包
均记录并遵循相应功能选项；具体命令见 [BUILDING.md](../BUILDING.md#optional-extensions-on-yuzu)。

构建验证结果保存在此前命名的本机忽略目录 `D:/pcsmash/build/main-integration-20261018/`；
此目录名只反映首次测试时的命名，不表示这些个人功能属于 main。构建与测试代码保持相同，只有分支归属、文档和仓库约定得到纠正。
检查覆盖四种组合的真实宿主编译/链接、功能与产物哈希、打包前置核对和原生手柄自测；
另外运行延迟计算/会话门控、核心并发、偏好保存、真实 WebView2 前端及本机构建开关回归。
四种实际打包结果也已核对：MOD 后端/目录与延迟插件/前端仅在各自启用时存在，关闭时不带入残留文件。
打包矩阵复用已有 Console 可执行文件，未重编 Console，并以 `--allow-stale` 允许该旧 Console；
宿主源码、功能选项、模块源文件与产物哈希已在此前严格核对。测试包不含 data.arc，不作为新发布包交付。
延迟计算对照为 2,125 例，独立核心为 1,088 例并含 16,000 次并发操作；MOD Rust 测试 9 项通过，
构建/打包与加载开关回归共 9 项通过。
这些自动检查本身不替代真实联机对局验收。后续用户已确认跨 Yuzu、PC 客户端之间的双人及三人真实测试通过，详见上面的当前状态补记。

## 远程状态

本次尝试刷新所有已配置远程。`origin` 的读取完成；`fork` 无法连接 GitHub，
`denkishi` 读取被连接重置打断，因此后两者只能按已有本地远程引用记录。
本次恢复本地 main 的上游副本位置，并更新本地 yuzu；没有推送远程或修改上游 PR。
