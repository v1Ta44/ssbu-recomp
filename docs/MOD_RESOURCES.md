# 资源包后端（mod-adapt，游戏 13.0.4）

资源包通过宿主文件系统的虚拟归档视图加载。原 `data.arc`、游戏模块、重编译游戏 DLL 和 ELF 均只读；后端不生成修改版游戏文件。磁盘输出只有用户配置和独立的补丁缓存。Console 另设模组管理页，使用同一后端配置和扫描结果。

## 使用

资源包扩展默认不编译启用；使用 `python scripts/setup.py --only link --with-mods` 启用，`--without-mods` 关闭。启用版的游戏程序旁需要 `ssbu_mod_backend.dll`。关闭版不编译、不加载该后端，资源文件读取直接走原版路径；资源管理接口返回 `feature_disabled`。现有 PC Hook DLL 与角色 SDK 不受这一开关影响。把每个资源包放在游戏程序上一级的 `mods/<包名>/`；包内直接保留归档路径。例如标准开发布局：

```text
build/
  work/ssbu_native.exe
  work/ssbu_mod_backend.dll
  mods/MySkin/fighter/mario/model/body/c00/...
  mods/MyText/ui/message/...
  config/mods.json
  cache/mods/<内容指纹>.blob
```

打包结果使用同样的父目录关系：**包根目录的 `mods/<模组名>/`，与 `game/` 同级**。例如 `<包>/mods/MySkin/fighter/...`，后端位于 `<包>/game/ssbu_mod_backend.dll`。不要放在 `game/mods` 或 `romfs`。下载包若有 `ultimate/mods/MySkin/` 外层，请取出其中的 `MySkin` 放入包根的 `mods/`。打包程序不会自动复制本机已安装的模组、配置或补丁缓存；启用包附带 `mods/README.txt`。

打包读取并验证 `ssbu_native.build.json` 中的宿主哈希和编译开关，不凭残留 DLL 猜测。关闭版不包含资源后端、其许可证或模组目录；启用版包含后端、构建指纹、许可证及可用的 PDB，并在 `manifest.json` 的 `features.resource_mods` 与 `mod_resources` 中记录状态和相对路径。旧构建需要重新链接以产生宿主构建记录。

环境变量可以指定其他位置：

| 变量 | 用途 |
|---|---|
| `SSBU_RESOURCE_MODS` | 资源包根目录；`0` 强制关闭此加载层 |
| `SSBU_MOD_PROFILE` | 后端配置文件，默认游戏程序上一级的 `config/mods.json` |
| `SSBU_MOD_CACHE` | 补丁缓存目录 |
| `SSBU_MOD_REGION` | 显式地区语言，例如 `us_en`、`zh_cn`、`zh_tw`；默认 `auto` |

`auto` 使用当前用户存档中的语言和宿主的语言选择。存档语言字节位置沿用 ARCropolis 的公开实现（`system_data.bin + 0x3c6098`），没有修改存档。没有存档时使用宿主语言。改变语言或资源包配置后重新启动游戏。

资源包和 `SSBU_MODS` 的宿主 DLL 模组分开管理；原角色 SDK 和 `SSBU_CHARACTERS` 的实验代码保持独立。

## 兼容行为

- 已有资源替换、新资源、目录和批量加载组；压缩和未压缩资源统一提供真实大小及内容。
- `stream;/...` / `stream:/...` 流式资源，支持已有条目替换和新增条目；分段读取无需把整个音乐或视频读入内存。替换流使用虚拟前置布局，保留原生音乐消费者的无符号 32 位寻址；原流与文件/共享数据区的虚拟位置同步调整，原文件不改写。流范围超限时准备阶段明确报错。
- 小写路径、Windows 路径分隔符、`+地区` 后缀、`.mp4`→`.webm` 和 `.lua`→`.lc` 的路径别名；别名不会转换媒体编码。
- 现有工具使用的 `0x<文件名 Hash40>.<扩展名>` 和无扩展名的完整 Hash40 路径。
- 默认独立替换共享资源；其他服装保留原内容。`keep-shared` 可显式保持共享。
- 音频替换只在配套音库确实需要解除共享时，带入原版 `.nus3bank` 并分配独立标识，以支持单服装语音。原本独立的音库（例如 common narration）保留原编号和全部字节；显式 `keep-shared` 同样阻止隔离。新编号从固定 ARCropolis 实现使用的 7420 开始。
- `config.json` 的 `unshare-blacklist` / `keep-shared`、`preprocess-reshare`、`share-to-vanilla`、`share-to-added` / `new-shared-files`、`new-dir-infos`、`new-dir-infos-base`、`new-dir-files`，以及其下划线别名。
- 共享目标支持单个路径、路径数组和公开 `arc-config` 的结构化文件/父目录形式。共享链会解析到最终来源，循环会报错。
- 额外服装、角色效果、舞台资源的加载组推断；相应的继承子目录由已存在的基础组提供。新增角色 EFF 由现有公共特效组统一持有，各颜色通过原生依赖加载，避免换色后枚举到未加载的特效；新增 EFF 使用原版分配对齐。
- 参数补丁：`prcx/prcxml`、`stdatx/stdatxml`、`stprmx/stprmxml`；消息补丁 `xmsbt`；音频补丁 `patch3audio`；动作列表 `motion_list.yml` 和 `motdiff`；`bgm_property.bin` 合并。

普通替换按优先级从低到高应用，较高优先级获胜；同优先级按包目录名排序，后者获胜。地区专用文件在同一个包的通用文件之后应用。补丁也从低到高合并，基底采用最终获胜的完整文件，没有完整文件时读取原归档。动作列表按 ARCropolis 的规则先应用完整 YAML，再应用 diff。冲突模式 `error` 可阻止同路径完整文件冲突。

每个补丁缓存记录输入指纹、输出长度和输出 SHA-256；命中后仍验证内容。缓存异常不会改变原归档。已打开的资源文件在 Windows 上保留只读共享句柄，运行中修改配置不会改变当前资源视图。

`plugin.nro` 是 Switch 可执行代码，本后端不执行它；发现这类依赖会在计划和日志中明确报告。现有原生 DLL Hook API 继续可用。本次提供的是资源文件兼容，不宣称现成 Skyline 插件、依赖插件的整包或额外角色槽位的游戏逻辑能直接运行。Console 模组页提供支持状态检测、内容标签、别名、启停、槽位映射、ZIP／文件夹导入、删除到回收区及目录入口；没有实现在线更新或运行中热切换。

## Console 管理与虚拟槽位映射

Console 的“模组”页检查宿主构建记录、实际 EXE 哈希和测试包清单，再确认后端能加载。关闭版保留页面并显示未启用，不会因残留 DLL 误判可用。

扫描结果包含每包 `resources` 原始资源路径和 `costumes`（例如 `sonic:c00`）。Console 由此标注角色、服装、资源类型及同路径覆盖；这些是按路径识别的替换／新增内容，不把识别结果当作完整兼容认证。带可执行插件的包单独提示。坏配置会显示计划错误，同时仍允许关闭或删除相关包。

别名和开关保存到用户 `config/mods.json`。`slot_map` 的键是“原角色:原服装”，值为目标服装的零基编号；例如 `{"sonic:c00":2}` 把 Sonic 第 1 套资源路由到第 3 套。模型、动作、服装语音／音效、头像、镜头、特效和可读路径配置按识别规则一起路由。所有磁盘来源路径和原始字节保持不变；映射结果通过虚拟归档和加载组提供。禁止把同包两套资源映射到同一个目标。插件包、纯哈希资源和不可还原的哈希配置不能自动改槽位；额外服装选择和游戏逻辑仍受前文兼容边界限制。

导入支持 ZIP 和已解压文件夹，可识别 `ultimate/mods` 外层，重名时创建独立目录；不覆盖已有包。导入前检查计划，拒绝越界路径和链接。删除移入模组根目录的 `.trash/`，可手动找回。修改操作与游戏启动互斥，游戏运行时只允许查看和打开目录。

保存配置时，包内路径写成相对于 `game/` 的路径，移动整个测试包后仍可使用。默认安装目录为 `<包>/mods/`，缓存和配置分开放置。字体子集同时收集前端和后端文案，并提供同款完整字库后备以覆盖用户的模组名和别名。

## 给前端的接口

公共 C ABI 在 `runtime/mod_resources.h`：

```c
uint32_t ssbu_mods_api_version(void);                /* 当前为 1 */
char *ssbu_mods_request(const char *request_utf8);   /* 返回独立分配的 UTF-8 JSON */
void ssbu_mods_free(char *response_utf8);            /* 所有返回值都用此接口释放 */
```

前端也可直接加载后端 DLL，使用 `sbm_api_version`、`sbm_request`、`sbm_free`。DLL 和宿主封装的分配器不同，不可混用释放函数。调用方应使用匹配的同一模块释放返回值。所有返回字符串由调用方拥有，错误也以 JSON 返回。

现有宿主控制管道增加一条后端命令：`mods<TAB><JSON><换行>`，返回一行 JSON。请求上限沿用管道的 16383 字节；响应动态分配。没有修改外部控制台前端。

| `op` | 参数及结果 |
|---|---|
| `capabilities` | API/游戏版本、功能清单、`plugin_nro=false`、`hot_reload=false` |
| `get_config` | 当前待生效配置 |
| `configure` | `config` 部分更新；不自动写盘 |
| `init` | 加载存在的配置文件，再应用指定字段；供宿主启动使用 |
| `scan` / `plan` | 资源包、元数据、插件依赖、文件来源、补丁链、共享规则、冲突和诊断 |
| `set_package` | `id`、可选 `enabled`、整数 `priority`、字符串 `alias`、`slot_map` 对象 |
| `save_profile` | 原子保存当前配置；文件必须使用 `.json` 扩展名 |
| `load_profile` | 可选 `path`，加载另一个配置方案 |
| `status` | 活动归档、统计、当前/活动配置、是否需重启、准备进度及失败原因 |
| `open` | `path`，为后端工具准备只读虚拟归档；游戏中由宿主自动执行 |

成功：`{"ok":true,"result":...}`；失败：`{"ok":false,"error":{"code":...,"message":...}}`。缺失或不支持的配置不会伪装成功。

配置示例：

```json
{
  "root": "D:/SSBU/mods",
  "cache": "D:/SSBU/cache/mods",
  "profile": "D:/SSBU/config/mods.json",
  "region": "auto",
  "userdata": "D:/SSBU/userdata",
  "system_language": "en-US",
  "conflict_mode": "priority",
  "packages": {
    "MySkin": { "enabled": true, "priority": 10, "alias": "我的皮肤", "slot_map": { "sonic:c00": 2 } },
    "OtherSkin": { "enabled": false, "priority": 0 }
  }
}
```

`status.progress.phase` 为 `idle`、`scan`、`materialize`、`metadata`、`ready` 或 `failed`。`done/total` 是资源准备数量，不是整个启动过程的百分比。耗时准备不持有控制接口的配置锁，前端可查询状态；读取操作使用不可变快照和带偏移的 I/O，不共享文件游标。

## 实现边界与构建

- `tools/mod_backend/`：扫描、配置、补丁处理、归档解析、共享关系、虚拟读取；动态容器，不沿用旧 loose 层的 4096 文件限制。
- `runtime/mod_resources.c`：加载独立 DLL、初始化配置、转换宿主路径、管理归档句柄。
- `runtime/nnfs.c`：仅在只读 `rom:/data.arc` 句柄上接入读取和大小查询；其他文件仍走原宿主实现。
- 虚拟归档保持原版本附加表，新增主表记录会统一重排、重定位。批量加载保留每个文件对应的数据槽位、16 字节填充及原分配对齐。

以 ARCropolis `bf2005b0282a524f242ae8977a70dad23e801318`、`smash-arc 0.6.0` 和仓库既有实现为参考。参考上游的版本检查是 13.0.5；本适配器固定为本仓库的 13.0.4，没有沿用上游游戏地址。

启用此扩展时需要 Rust/Cargo（本机验证 Rust 1.91，x64 Windows MSVC 工具链）。`Cargo.lock` 固定依赖，构建使用 `--locked`。补丁库沿用 ARCropolis 使用的公共解析器。只有指定 `--with-mods` 时，`scripts/setup.py` 的 `link` 步骤才构建后端；开发时可单独执行：

```powershell
python scripts/build_mod_backend.py
```

输出 DLL、构建指纹和依赖许可证到 `build/work`。打包会收入它们，并拒绝与源码指纹不匹配的后端。运行包不需要安装 Python 或 Rust。正常链接支持 `--host-only`，只重建宿主，复用已有游戏库。

## 验证与实机交接

快速验证：

```powershell
cargo test --locked --manifest-path tools/mod_backend/Cargo.toml --target-dir build/mod-adapt/cargo
python scripts/build_mod_backend.py --debug
.venv/Scripts/python.exe tests/test_mod_backend.py
```

独立归档检查默认使用 `build/state.json` 中的私人归档；也可指定 `--arc` 和 `--backend`。测试只生成 `build/mod-adapt/reader-fixtures/` 下的标记资源，不启动游戏，也不写原归档。这些标记文件不能作为可玩的模型或纹理包。

2026-10-18：用户已逐组确认本地 GameBanana **2～9 号测试通过**，包含 11 个包的单项与合并运行。覆盖皮肤/加载图、单服装语音、文字与参数及音频补丁、动作与特效、角色新增资源及换色重载、舞台、音乐/视频，以及合并后的对局和退出重进。详细结论与原始证据索引见 [验收记录](MOD_GAMEBANANA_TEST_2026-10-17.md)。收尾没有继续修改或重建已验收代码。

离线检查包括七项快速 Rust 单元测试，以及针对共享/地区音库、独立播报音库、特效加载依赖和原生音乐 32 位寻址的聚焦回归。普通归档集成覆盖共享链、搜索表、版本附加表、批量读取顺序、并发读取与配置快照。真实合并配置覆盖 322 个资源目标和 50 个加载组；这些检查与用户实机反馈分别记录，不互相替代。

本轮验收不代表所有 ARCropolis 包都能运行。带原生 Switch 插件的包、额外角色槽位的游戏逻辑、所有地区与补丁格式的实机覆盖及长期稳定性不在本轮通过结论内。英语合并消息/流准备已离线验证；没有独立英语运行记录时，不声称英语实机已验证。无效 Knuckles 原包 XMSBT 被明确跳过，未改写原模组。该轮资源验收时没有新增前端；后续 Console 管理页见上文。

可回溯检查点记录在 `docs/checkpoints/MOD_ADAPT.md`；Git 引用在 `refs/checkpoints/mod-adapt/`，不需要切换最终分支就可检查各阶段快照。


## 真实资源包的手动测试入口

2026-10-17 下载并校验的 11 个 GameBanana 资源包已于 2026-10-18 完成本轮用户 2～9 号验收。可继续打开本地 `build/mod-adapt/gamebanana/09-全部合并.cmd` 使用已验收的全包配置；各阶段操作、故障修复、证据位置和未覆盖能力见 [GameBanana 测试清单](MOD_GAMEBANANA_TEST_2026-10-17.md)。第三方压缩包和安装文件只存于忽略的本地目录，不随源码或发布包分发。

真实包中发现的坏 XMSBT 会在扫描阶段按 ARCropolis 行为跳过，`plan.diagnostics` 与运行日志给出 `invalid_xmsbt`，不会因此丢弃整个皮肤包；其他有效文字补丁仍叠加。原版消息文件解析失败或其他资源错误仍明确报错。


## 历史排查记录

以下保留修复过程中的状态，当前验收结论以上文及测试清单页首为准。

2026-10-17 用户测试反馈后的修复：虚拟加载组搬移时保留完整相邻地区/语言组，防止原生调度器把角色脚本加载指向其他组。独立读取检查现包含这一原生索引约束；旧 DLL 失败、修复 DLL 通过。手动资源测试启动器位于 `scripts/play_resource_test.py`，显式设置 `SSBU_VOLUME=100`，避免测试 EXE 名称触发默认静音。下一轮先复测 2 号加载和声音，具体步骤见上述测试清单。


第二轮 2 号测试已加载 Sonic 脚本，随后在首次贴图读取中崩溃。现场确认读取失败，尚未确认源地址失效的原因。候选补丁在初始化时取得 packaged 贴图数据的独立副本，直到首次上传完成；其目的是消除这一条延迟借用，尚不能宣称已修复原游戏崩溃。专用检查 `tests/test_nvn_packaged_texture.py` 使用真实初始化/哈希/解码路径验证源缓冲区释放后的完整 mip 数据、上传后释放、未使用贴图清理及初始化失败处理，不启动游戏或 GPU。独立宿主已更新，实际对战由用户继续复测。


当前验证状态更新（2026-10-17 14:00）：第三轮 2 号在启动阶段发生 `0xC0000374` 宿主堆损坏，前述贴图副本修复尚未通过实机验收。已保留原现场和失败宿主，尚未证明具体损坏调用。2 号启动器接入仅针对其子进程的异常捕获，以取得 fail-fast 发生时的新转储，其他组不受影响。捕获链路通过独立原生小程序验证；未启动游戏，未将诊断准备当作故障修复。详情见测试清单最上方的当前状态。

生命周期复查（2026-10-17）：早先计算回写缺陷有纯上游复现证据；贴图读取失败与最新启动堆损坏仍须分别定因。两个现场尺寸的带边界保护离线检查通过，未验证实际渲染并发。详见 [运行时生命周期复查](MOD_RUNTIME_LIFETIME_AUDIT_2026-10-17.md)。本轮未修改运行时或启动游戏。


## 2026-10-19 prebuilt 资源与启动错误处理

资源路径支持 ARCropolis 的 `prebuilt;` / `prebuilt:` 前缀，规范为 `prebuilt:/` 并保留完整前缀参与 Hash40；它与 `stream:/` 分开处理。例如原版 13.0.4 归档中已有 `prebuilt:/movie/fighter/brave/c00/final_00.h264`，按普通归档文件替换，不能改名为 stream 或删除前缀。仍拒绝未知命名空间、目录穿越与资源名内额外冒号。

扫描错误附带包 ID 和相对资源路径。模组后端无法准备 data.arc 时，宿主记录 `MOD RESOURCE ERROR` 并以退出码 8 明确停止启动；Console 与旧版日志启动器给出模组加载失败说明，避免把无效资源继续交给游戏而触发空指针。不会静默禁用模组或退回无模组运行。

回归入口：模组后端 Rust 单元测试与 `tests/test_mod_prebuilt.py --backend <新 DLL>`。后者需要本机原版归档，使用独立读取器验证实际资源字节替换，不分发游戏资源。
