# 便携测试包更新功能（2026-10-19）

已在个人 yuzu 分支实现，不修改上游闪烁修复 PR。

## 用户操作

开始页新增“版本与更新”：手动检查、从文件更新、版本说明、下载来源、下载并准备、退出并安装、恢复上一版本。启动检查默认关闭，可由玩家主动打开。检查失败会提示暂不可用，已有版本仍可启动；不会把失败当成最新版本。

直链由 Console 下载，损坏或失败时尝试同清单其他直链；浏览器分享页只在浏览器打开，用户下载后导入。提取码和来源说明按签名清单显示，不内置任何发布者账号或令牌。高级设置可配置最多四个 HTTPS 清单入口；所有入口必须通过同一组内置 Ed25519 公钥校验。尚未验证的网盘、Gitee、AtomGit 等候选不冒充可用镜像。

对旧测试包，先关闭游戏和 Console，将新版热补丁文件夹放到包根目录，与 game 并列，双击 Apply.cmd。已安装上一版闪烁热补丁的完整测试包也可直接安装。Rollback.cmd 恢复这一次安装前的程序；Recover.cmd 用于中断恢复。后续可直接在 Console 导入发行的签名更新 ZIP，校验通过后再点击退出并安装。

## 实现与版本协议

- `tools/updater`：共享库、原生辅助程序；Ring Ed25519 验证、SHA256、ZIP 路径和大小限制、暂存、基础文件核验、备份、替换与恢复。
- `tools/console/src/updates.rs`、`ui/updates.js`：后台检查/下载/校验与版本卡片。网络操作不阻塞界面，UI 轮询的是本地进度，不轮询远程服务。
- `scripts/release_update.py`：从已核验测试目录和可选新文件覆盖生成完整文件补丁、完整更新 ZIP、可选首次使用便携 ZIP，以及同一版本的签名通道清单。
- `scripts/package.py --public`：明确排除 data.arc、缓存、作者/测试存档和本机配置，清单移除机器名、原始绝对路径、符号存储位置；保留已有构建过期检查。

签名信封是 `{key_id, payload, signature}`，payload 为原始 JSON 字节的 Base64，签名覆盖这些字节。版本包含 schema=1、channel=yuzu-test、platform=win-x64、递增 sequence、可读 version、build_id、game_version=13.0.4、资源模组/延迟滑块功能组合、最低更新器版本。

通道清单附版本说明及完整/补丁附件的大小、SHA256、适用基础 build、直链或浏览器来源。补丁内的 update.json 另外签名文件替换/删除允许列表、逐文件大小/哈希和各基础 build 的文件前置哈希，离线导入同样核验。补丁基础不匹配时停止，在线选择器优先匹配补丁，否则选择兼容完整包；本地已修改文件不会被静默覆盖。

发布序号用于新旧比较，build_id 用于具体构建匹配，文件哈希用于确认实际基础内容；不依赖 Git 提交号或 Console 自身 0.1.0。完整/补丁更新属于完整文件替换，不是二进制差分。

## 安装、并发与恢复

Console 内嵌编译好的辅助程序，在暂存目录写出并核验后启动；不从远程下载可执行安装脚本。辅助程序等原 Console 退出后取得目录独占锁，再核验签名、基础、文件占用和剩余空间。相同目录的第二个新 Console 或更新器不能并行操作；旧 Console、游戏或 DLL 被占用时文件检查/替换会停止。

只允许发行程序、配套 ELF/.nrr、系统字体和明确列出的配套文件。存档、模组、用户配置、缓存、日志、报告、data.arc、延迟面板 WebView 数据不在替换范围；不会清空 game 或 romfs 目录。目标路径禁止穿越、绝对路径、ADS、Windows 保留名及链接/联接点；ZIP 禁止异常链接、重复/额外条目，限制元数据、文件数、压缩包及解压总体积。

先复制并核验备份，写入持久安装日志 `updates/transaction.json` 后才开始替换；使用同步临时文件和重命名替换，避免改写旧包与符号存储的硬链接。替换失败自动恢复；进程中断由下次启动或 Recover.cmd 恢复。多文件安装不是断电原子事务，恢复记录和备份必须保留。备份损坏或安装后文件被用户改动时，撤销操作拒绝覆盖。

成功后保存原始签名安装记录及发行版本，重新打开 Console。恢复上一版本会一起还原版本、清单、旧程序与上次签名记录。成功/失败结果保存在 updates/last-result.txt，恢复失败不会循环重启 Console。

## 构建与发行

先构建辅助程序，再构建内嵌该程序的 Console：

```powershell
$env:CARGO_TARGET_DIR = "$PWD/build/updater-target"
cargo build --manifest-path tools/updater/Cargo.toml --release --no-default-features --locked
$env:CARGO_TARGET_DIR = "$PWD/build/console-target"
cargo build --manifest-path tools/console/Cargo.toml --release --locked
```

签名公钥在 `tools/updater/trust.json`，已编入两者。发行私钥保存于本机 `C:/Users/haoye/.codex/private/pcsmash-yuzu-release-20261019.pk8`，不在仓库或发行包内。后续发行沿用该密钥；不要把私钥提交或打包。

`release_update.py --help` 列出完整参数。--package 是经过核验的完整目录，--overlay 只放发生变化的发行文件，--base 可重复指定原包及已打补丁的基础目录；基础目录必须完整，不能用缺文件的观察样本代替。--sequence 必须递增，--version 和 --build-id 为新发行标识。--sources JSON 逐项区分 direct/browser，URL 可含 `{filename}`。--portable 可从签名完整包生成首次使用布局，默认设置是发行模板，不含作者配置。

主清单入口预置为个人 fork 的固定 yuzu-test 发行标签下的 update.json，支持预发行，不调用 /releases/latest。此次仅生成、核验本地发行材料，未上传二进制、未发布在线通道。只有维护者上传同一份附件、核验已同步来源后再发布签名 update.json，在线检查才有实际发行可用；此前 404/离线会如实提示，并可使用离线导入。

## 验证

21 项核心测试（14 项事务/文件测试，7 项来源/下载测试）涵盖签名/不受信发布者、损坏/额外归档条目、路径限制、功能/基础不匹配、未变更受管理文件被改动、个人文件保留、Windows 文件占用/目录锁、安装失败回滚、中断恢复、备份损坏、后续版本撤销与硬链接保护。

来源测试用注入的响应验证主源失败、较旧镜像、最高有效签名版本、损坏下载切换、浏览器来源不直下、取消、HTTPS 限制和完整包回退；不据此宣称大陆镜像已实网验证。Console 全部 16 项单元测试通过（已配置真实模组后端与测试包，包含原来按需忽略的两项），验证运行中阻止安装及来源设置限制。原有模组、名字设置、控制器等测试一同运行。

真实控制台验收记录在 `work/updater-release-20261019/ui-results.json`：实际 HTTPS 入口失败时显示不可用，签名离线导入，控制台退出，实际替换自身程序，自动重启，恢复上一版本并重启。实际旧完整包与上一版闪烁热补丁完整目录的安装/撤销/重新安装、逐文件哈希和个人文件/硬链接保留记录在同目录 install-results.json。用于验收的序号 2 发行仅保存在 work 下，不会发布。

最终发行大小、哈希与最后核验记录在本文后续补充；旧闪烁热补丁原文件保留。

暂存目录和临时文件的标识使用进程号、时间和进程内单调序号，新增 8000 个并发标识唯一性验收。手动热补丁由 scripts/package_update_hotpatch.py 生成；外层 ZIP 提供原生安装器和源码，内层 Update.zip 是 Console 可导入的签名文件。


## 最终交付与验收结果

源码提交 `741637aa8b3679fcedc96c8e879dbe1eab51dee0`（本地 yuzu）。编译的 Console 内嵌辅助程序与发行 helper 的 SHA256 一致；热补丁源码归档采用该提交，运行时复用旧包已验证的基线加三处修改，并附实际 staged 源码覆盖与逐文件构建溯源。

- 手动热补丁：`D:/pcsmash/build/dist/FlickerHotpatch-20261019-with-updater.zip`，19966102 字节，SHA256 `34cdc73387c832b1122eed8802cebdea01c90917afa3304b84c0b4f95b7b96eb`。包含原闪烁/失焦修复、新 Console、更新辅助程序、源码和中文说明。原版闪烁 ZIP 未覆盖。
- 签名文件补丁：`D:/pcsmash/build/dist/yuzu-update-20261019/yuzu-test-2026.10.19.1-patch.zip`，11736689 字节，SHA256 `658648920d7cd32b479d916e276370ecbec8c9f1c941e067740c546666d38029`。
- 签名完整更新包：`D:/pcsmash/build/dist/yuzu-update-20261019/yuzu-test-2026.10.19.1-full.zip`，343025649 字节，SHA256 `b9acbc3a8eae39a83d24790f80423aed3702f38d1d5b3b2b73eda69afa06ec6c`。
- 首次使用便携完整包：`D:/pcsmash/build/dist/yuzu-update-20261019/yuzu-test-2026.10.19.1-portable.zip`，343041978 字节，SHA256 `f3e9638fc1c7c8e4c999fdba9ca5f3762b6e084ad482f9057ecfdd50df1e4091`，不含 data.arc、缓存、作者/测试存档。完整文件和签名版本已核验；使用者自行提供 data.arc。

发行序号 1，可读版本 2026.10.19.1，build 6525ebfdd221-fp6-bg-upd1。支持原 build 6525ebfdd221 和上一版 6525ebfdd221-fp6-bg；资源模组、延迟滑块均开启，游戏版本 13.0.4。内层 Update.zip 可供已有更新功能的 Console 导入；外层 ZIP 用于旧 Console 首次手动安装。

最终验证：21 项 updater 测试通过，16 项 Console 测试通过，未忽略已配置的真实模组后端测试。最终产物两种基础的安装、逐字节回滚、重新安装、发行清单所有哈希、个人文件保留和原始硬链接内容通过；完整文件更新与撤销也实际执行通过。已从最终外层 ZIP 检查全部文件 SHA256、源码 CRC、无私钥；实际双击入口的 cmd 安装/还原成功。首次使用 ZIP 的每个受管理文件与签名完整包一致，清单无本机路径，未带 data.arc、缓存或个人存档。

真实最终 Console 再次完成 HTTPS 失败提示、签名离线导入、退出后替换自身、自动重启、恢复上一版再重启，浏览器页面错误 0。界面已渲染检查，网络错误细节折叠显示。未进行新的实际联网房间对战或 RTX 3050 视觉复测；原闪烁修复的宿主二进制哈希与上一轮已验收版本相同。

结果文件：work/updater-release-20261019/updater-tests-final.log、console-tests-final.log、install-results-final.json、ui-results-final.json、release-audit-final.json。UI 图为 ui-version-card-final.png、ui-ready-card-final.png。完整发行清单及附件记录在 build/dist/yuzu-update-20261019。在线通道未发布，实际 GitHub 入口当前不可用已被正确识别，不显示为“已是最新版本”。


## 首次在线通道发布：2026.10.19.2

2026-10-19 16:09（Asia/Shanghai）已将模组 prebuilt 兼容修复发布至个人 fork 的 yuzu-test 预发行通道；此前“仅本地生成、未上传”的描述对应 2026.10.19.1 的交付状态。现预置 update.json 入口真实可用。序号 2 支持原测试包、闪烁/后台补丁及更新功能首版三种基础；补丁约 12.8 MiB，附签名完整文件更新与源码。真实旧版 Console 的在线检查、下载重试、签名校验和准备已通过，原生安装及自动重启后版本为 2026.10.19.2。详见 MOD_PREBUILT_UPDATE_2026-10-19.md。
