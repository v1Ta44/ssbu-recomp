# 项目接口说明与检索入口

核查日期：2026-10-16。对象：当前工作区源码，以及当前 `build/work` 的本地构建符号。

本次索引覆盖 329 个源码文件、95,440 行：1,822 条函数声明/定义记录，60 条函数指针约定，1,810 条游戏导入/NVN 注册候选记录（1,617 个不同名称），498 个环境变量名，59 条 argparse 参数，173 个 Python 入口，27 条默认配置字段，以及 100 个本地模块的 99,017 条符号记录。当前 EXE/游戏 DLL/NRO DLL 共读取 117 条 PE 导出记录。以上各项计数口径不同，不应相加理解为公共 API 数量。

直接打开 [离线检索页](../build/interface-audit/index.html) 或 [结构化完整清单](../build/interface-audit/interfaces.json)。

## 1. 先确定这里的“接口”是什么

本项目是 SSBU 13.0.4 的 Windows 原生静态重编译工程。仓库未发现面向浏览器的 HTTP / REST / WebSocket 服务端路由，也没有 OpenAPI 定义。源码中的 `http://localhost/` 是离线网页小程序的模拟返回内容，不是正在监听的服务地址。

接口分为以下几层；它们不能直接用浏览器或 Postman 调用。

| 层 | 谁调用谁 | 入口 |
|---|---|---|
| 联机公共 ABI | 同进程的控制窗口或其他原生前端 → 联机后端 | `runtime/yuzu_net.h` |
| 联机线协议 | 本程序 → Yuzu 房间服务器及同房成员 | `runtime/yuzu_net.c`、`yuzu_ldn.c`、`yuzu_socket.c` |
| 模组接口 | 运行时 → 模组 DLL；模组 → Hook 表 | `fighter_sdk/include/hook.h` |
| 新角色注册 | 角色组件 → 运行时的角色清单和代理工厂 | `fighter_sdk/include/registry.h` |
| 角色脚本 SDK | 可编辑角色 C++ → 游戏函数及 Lua 兼容层 | `fighter_sdk/include/fighter_sdk.h` |
| 游戏系统导入 | 重编译游戏 → Windows 侧系统库实现 | `runtime/hostrt.h` 和各 `ImportDef` 注册表 |
| 图形分发 | 游戏获取 NVN 函数指针 → OpenGL / Vulkan 前端 | `runtime/nvn_stub.c`、`nvn_gl.c` |
| 模块装载 ABI | EXE ↔ 游戏 DLL、脚本模块 DLL、角色 DLL | `game_bind`、`nro_bind`、`fighter_bind` |
| 运行和配置 | 用户 / 启动器 → Python 命令、环境变量、INI 和清单文件 | `scripts/`、`tools/package/`、`BUILDING.md` |
| 内部协作接口 | 输入、渲染、资源、异常处理等组件互相调用 | `runtime/*.h` 与跨文件函数 |

## 2. 完整清单放在哪里

运行以下只读扫描；扫描器只读取源码和本地符号，不启动游戏、不连接服务器、不读取环境变量实际值：

```powershell
.\.venv\Scripts\python.exe tools/audit/interfaces.py
```

输出在 `build/interface-audit/`：

| 文件 | 内容 |
|---|---|
| `index.html` | 离线检索页；按接口名、符号、文件、配置名搜索，可按类别筛选 |
| `interfaces.json` | 全部结构化明细、扫描文件 SHA-256、Git 版本和统计口径 |
| `source_functions.jsonl` | 头文件声明、跨文件函数和显式导出函数；含签名和位置 |
| `callback_contracts.jsonl` | 函数指针、回调及函数表的签名 |
| `header_macros.jsonl` | 头文件的函数式宏，包含脚本包装入口 |
| `guest_imports.jsonl` | 游戏导入候选项 / NVN 分发表项 → 处理函数、实现位置和正文摘录 |
| `dynamic_bindings.jsonl` | 动态查找、SDK 替换和运行时函数地址注册位置 |
| `generated_symbols.jsonl` | 当前本地 main / SDK ELF、生成 NRO 模块的导入和导出符号 |
| `binary_exports.jsonl` | 当前 EXE、游戏 DLL、NRO DLL 的实际 PE 导出 |
| `environment.jsonl` | 环境变量名称及源码使用位置；包括读取、写入和辅助函数参数 |
| `cli.jsonl` | argparse 参数、默认值、入口脚本与手动解析参数的源码证据 |
| `configuration.jsonl` | 图形 / 控制器默认 INI 字段与源码位置 |
| `SUMMARY.md` | 本次扫描统计与限制 |

只扫描源码可加 `--no-build-symbols`；通过 `--out` 指定其他输出目录。输出含从本地游戏副本提取的符号，保留在被 Git 忽略的 `build/` 下。

**统计口径：** 声明和定义可能分别列出；同一符号的多次注册也分别列出。历史 `r1src`、测试工具及模组组件按路径保留，不等于默认 EXE 的公共 API 数量。生成符号表包含函数和数据，不能把所有符号当成可直接调用的函数。

## 3. 联机公共 C ABI：9 个函数

定义：`runtime/yuzu_net.h`。实现：`runtime/yuzu_net.c`。ABI 版本为 `SSBU_NET_ABI = 1`，Windows 下显式 `dllexport`。已读取当前 `ssbu_native.exe` 的 PE 导出表，下面 9 项均存在。

| 函数签名 | 输入 / 输出约定 | 返回及行为 |
|---|---|---|
| `int ssbu_net_join(const SsbuNetConfig *config)` | `size` 必须等于当前结构体大小，`abi=1`；必填 host、合法 nickname；字符串均须在数组内以 NUL 结束 | 0 表示异步任务启动成功；参数错误 / 忙 / 线程失败立即返回错误。实际进房结果继续查 snapshot |
| `void ssbu_net_leave(void)` | 无参数 | 停止并等待后端工作线程；DNS 解析可能使等待变长，控制窗口应在工作线程调用 |
| `int ssbu_net_snapshot(SsbuNetSnapshot *out)` | 调用前设置 `out->size` 和 `out->abi` | 0 成功复制快照；参数不符返回 BAD_ARGUMENT |
| `int ssbu_net_set_log_level(uint32_t level)` | 0 关闭，1 生命周期，2 调试，3 数据包元信息 | 0 成功；大于 3 返回 BAD_ARGUMENT |
| `uint32_t ssbu_net_read_log(uint64_t *cursor, char *out, uint32_t capacity)` | cursor 从 0 开始，容量至少 1100 字节 | 返回复制的字节数，不含结尾 NUL；参数无效也返回 0。缓存最近 128 行，落后读者从仍保留的最老行继续 |
| `int ssbu_net_set_profile(const char *utf8_name)` | 合法 UTF-8，1～32 字节；这是游戏内名字，区别于房间 nickname | 0 成功；参数不合法或当前连接 / LDN 状态不允许修改时返回错误 |
| `void ssbu_net_get_profile(char out_name[33])` | 提供至少 33 字节缓冲区 | 复制游戏内名字；空指针直接忽略 |
| `int ssbu_net_set_avatar(uint32_t format, const void *bytes, uint32_t size)` | 0 清空；1 PNG；2 JPEG；最多 256 KiB；检查签名字节并复制 | 0 成功；参数、内存分配、当前状态可能导致失败。仅本地前端头像，未实现上传到房间服务器 |
| `uint32_t ssbu_net_get_avatar(uint32_t *format, void *bytes, uint32_t capacity)` | format 可空；缓冲区容量足够才复制 | 总是返回所需图片字节数，可先询问大小再分配 |

`ssbu_net_boot(void)` 是环境配置启动入口，没有公共导出标记，不属于上面的 9 个公共 API。

### 3.1 配置结构

`SsbuNetConfig` 主要字段：

| 字段 | 大小 / 规则 |
|---|---|
| `size, abi` | 结构体大小及版本；需使用匹配的头文件编译 |
| `host[256]` | 非空服务器地址；不是 HTTP URL |
| `port` | 主机字节序；0 采用 UDP 24872 |
| `nickname[21]` | 4～20 个 ASCII 字母、数字、空格或 `._-`；用于房间身份 |
| `password[128]` | 房间密码；无需密码时留空 |
| `token[4096]` | 可选服务器身份 token；不要求每个房间都有 |
| `game_name[128], game_version[32], game_id` | 游戏资料；环境启动默认 SSBU / 13.0.4 / `0x01006a800016e000` |

`SsbuNetSnapshot` 提供 state、error、server_version、revision、收发/丢包计数、自己的虚拟 IPv4、房间资料和最多 254 个成员。成员包含房间昵称、用户名、显示名、头像 URL、IP 和游戏资料。不要把“容量上限 254”理解成已测 254 人游戏对战。

### 3.2 状态和错误码

状态值依次是 `IDLE=0`、`CONNECTING=1`、`JOINING=2`、`JOINED=3`、`FAILED=4`。只有 `JOINED` 才表示已经通过服务器准入；进入游戏中的本地无线会话还有独立的 LDN 状态。

错误码按头文件顺序为：`OK=0`、`BAD_ARGUMENT=1`、`BUSY=2`、`IO=3`、`TIMEOUT=4`、`VERSION=5`、`PASSWORD=6`、`NAME=7`、`FULL=8`、`BANNED=9`、`KICKED=10`、`CLOSED=11`、`MALFORMED=12`、`OVERFLOW=13`、`IP=14`。

公共 API 线程安全，无前端回调。配置 / 名字 / 头像在连接进行中、已进房或 LDN 状态大于 1 时不可随意修改。`Leave` 后如游戏仍在本地无线会话，应先退出游戏内会话再重连。

最小调用流程：清零配置 → 设置 size / abi / host / nickname → Join → 循环 Snapshot → 等到 JOINED 或 FAILED → 游戏内进入本地无线模式 → 退出游戏内会话 → Leave。`SsbuNetConfig` 含密码和 token，前端自己的配置副本也应妥善处理。

## 4. 联机线协议与内部接口

这是 ENet 承载的 Yuzu v1 房间协议，默认 UDP 24872。整数在线报文中按大端写入，字符串为 4 字节长度加正文；具体 LDN 结构内部另有小端字段，不能把整个结构按一种字节序强转。编码见 `runtime/yuzu_packet.h`。

| 消息号 | 方向 / 用途 | 本地处理位置 |
|---|---|---|
| 1 | 客户端 → 服务器：Join；nickname、请求 IP `0xffffffff`、协议版本 1、password、token | `yuzu_net.c:room_worker` |
| 2 / 24 | 服务器 → 客户端：普通 / 管理身份准入成功，携带虚拟 IP | `yuzu_net.c:received_locked` |
| 3 | 服务器 → 客户端：房间元数据和完整成员表 | `yuzu_net.c:room_info_locked` |
| 4 | 客户端 → 服务器：游戏名、game_id、版本 | `yuzu_net.c:room_worker` |
| 5 | 双向：代理数据报；两端 IPv4/端口、协议、广播标志、Zstandard 压缩负载 | `yuzu_socket.c` |
| 6 | 双向：LDN 发现和连接控制，类型、源/目标 IP、广播标志、负载 | `yuzu_ldn.c` |
| 8 / 9 / 10 / 11 | 昵称拒绝 / IP 冲突 / 版本不符 / 密码错误 | `yuzu_net.c:received_locked` |
| 12 / 13 / 15 / 16 | 房间关闭 / 已满 / 被踢 / 被封禁 | 同上 |

未处理的聊天和管理消息不会因此成为本地可调用 API；没有公共房间发现、聊天或服务器房间管理 API。

LDN 子类型为 0 扫描请求、1 网络信息响应、2 节点加入、3 同步网络信息、4 节点离开、5 主机断开节点。固定网络信息为 `0x480` 字节，节点 `0x40` 字节；最大缓存 64 个扫描结果，每网最多 8 节点。

内部声明在 `runtime/yuzu_net_internal.h`：

- `yn_ldn_*`：初始化/结束、状态/事件、Station/AP 开闭、创建/连接/断开、信息/配置/IP/安全参数、广播数据、扫描、接纳策略、拒绝与断线原因。返回 Switch 风格 Result，不能按公共 `SSBU_NET_*` 枚举解释。
- `yn_socket_*`：create、close、close_all、bind、connect、name、recv、send、fcntl、poll、option、shutdown。虚拟 FD 从 `0x10000` 起；内部错误为负 guest errno，经 SDK 适配层转成游戏接口的错误形式。
- `yn_*_locked`：要求已持有同一个后端锁。房间、LDN 和虚拟 socket 共用锁；等待条件变量时释放锁，调用方应遵循声明约定。

能力边界：虚拟数据通道仅 IPv4 UDP，TCP/IPv6 返回不支持；非虚拟 socket 按适配逻辑转交原 WinSock 路径，不代表原 WinSock 实现本身不存在 TCP。虚拟 socket 数量上限 128；单个解压数据报最多 65507 字节；发送/接收总负载分别受 8 MiB 限制，单 socket 另有容量和 256 包限制。接纳策略仅实现 AcceptAll / RejectAll，Private 入口不等于完整实现原机安全策略。未连接 Nintendo 官方在线服务。

说明与现有测试证据：`docs/YUZU_NETWORKING.md`。接口审查本身未进行真实双客户端发现/对战测试；后续用户于 2026-10-18 确认跨 Yuzu、PC 客户端之间的双人及三人真实测试通过，详见该文档第 15 节。源码审查、协议自测与用户真实测试的证据范围分别保留。

## 5. 模组 Hook API

约定：模组 DLL 放到 `SSBU_MODS` 指定目录，导出：

```cpp
extern "C" __declspec(dllexport) void mod_init(const HookApi *api);
```

宿主在游戏开始前调用一次。`HookApi` 的 size / version 应核对，当前头文件版本为 1。

| 成员 | 参数及结果 |
|---|---|
| `install(address, replacement, user, orig)` | 按游戏地址安装；返回大于 0 的 id，0 失败；orig 得到安装前行为 |
| `install_name(name, replacement, user, orig)` | 按修饰后的导出名安装；模块未载入时可延迟安装，因此 orig 的存储必须长期有效，例如 static 变量 |
| `remove(id)` | 停用对应 Hook |
| `enable(id)` | 重新启用 |
| `calls(id)` | 查询调用次数；实现：`fighter_sdk/hook_host.c` |
| `address(name)` | 查已加载模块符号的游戏地址；未知为 0 |
| `call(ctx, address)` | 按地址调用游戏函数，参数和结果放在 ctx |
| `log(text)` | 写一行运行日志 |
| `error()` | 读取最后一次安装失败原因 |

`HookFn` 是 `void (*)(Ctx *, void *)`，`HookOrig` 是 `void (*)(Ctx *)`。整数/指针参数在 `x[0..7]`，浮点在 `v[0..7]`；结果在 `x[0]` 或 `v[0]`；返回 L2CValue 的间接结果缓冲区由 `x[8]` 提供。SDK 头文件里的 Ctx 只是宿主结构前缀，禁止按这个不完整定义自行分配。

首次 Hook 会改写函数开头的 5 字节，应在 `mod_init` 或没有游戏线程进入目标函数时安装；不要据此宣称所有安装动作都可在任意时刻安全发生。地址和偏移对应游戏 13.0.4。

## 6. 新角色注册与角色脚本 SDK

`ssbu_registry()` 是 EXE 导出的角色注册入口；`ssbu_registry_get()` 是头文件中使用 `GetProcAddress` 取得它的帮助函数，缺少入口时返回 NULL。

`SsbuRegistry` 包含 size、version、count、fighters，以及以下函数表：`start()`、`agent_begin(name)`、`agent_end(name,agent)`、`owner_set(name)`、`agent_begin_weapon(name)`、`article_agent(ctx,name,article,category)`。尾部成员分别由版本 2 / 3 / 4 扩展，使用前应核对结构大小。`article_agent` 返回 1 表示在 ctx 的 x0 中提供了代理，返回 0 表示继续使用模板代理。

角色清单入口是 `SSBU_CHARACTERS/<子目录>/fighter.json`。核心字段为 name、donor、dll，可带 kind、display_name、own_name 以及后续角色/武器资料。运行时解析并非严格通用 JSON Schema 校验：例如 kind 的读取经过字符串解析分支，应以 `new_fighter.py` 生成清单和 `newmodule_host.c` 的实际读取逻辑为准。角色资源路径遵循游戏归档路径，具体流程见 `docs/NEW_FIGHTER.md` 和 `docs/MODDING.md`。

角色脚本 ABI：`FsHost` 提供 call、log、ctx、call_addr；`fighter_bind(const FsHost *, FsTable *)` 返回符号槽、地址表及脚本替换表。`fs_function_at(i,off)` 按序返回函数名与模块偏移；越界为 NULL。`fs_module_address` 通过 SDK 宏按需导出。

脚本编写层包括 Hash40、LuaConst、L2CValue、L2CAgent、gcall / gcall_value、Lua 压栈/弹栈、`ACMD`、`L2C_FUNCTION*` 和 `L2C_VOID_FUNCTION*`。游戏使用 LP64，Windows 使用 LLP64，因此应坚持定宽整数类型，不用 Windows 的 long 代替游戏 long。

`lua_bind.h`、`lua_const.h`、`fs_syms.inc`、`bindings.inc` 属于本地生成文件，不在仓库中。本次检查的 `build/work/fighter_sdk` 也没有前三项，不能从现有 SDK 头文件虚构其所有包装函数签名；当前游戏和 NRO 的可用符号名称另收入本地生成符号清单。

## 7. 游戏系统导入：为什么同名接口会出现多次

`runtime/runtime.h` 定义寄存器上下文 Ctx、FnEntry、DynSym；`hostrt.h` 规定 `ImportFn = void (*)(Ctx *)` 和 `ImportDef = {name,fn}`。这种函数在宿主 C 层返回 void，游戏层的参数/结果实际经过寄存器和指针缓冲区传递。

核心入口：`register_imports`、`import_lookup`、`import_set_address`、`import_data`、`hostfn_address`、`call_guest`、`call_indirect`、`host_import`。guest 内存按真实地址映射，因此 guest 指针可以作为宿主指针使用，但必须满足宿主的生命周期和内存布局约定。

重要地址区间：主游戏 `0x7100000000`；导入占位地址 `0x7300000000`，槽间隔 256；运行时宿主函数地址 `0x7400000000`；SDK 映射地址 `0x7500000000`。NRO 模块地址由本地生成的 `nro_registry.c` 提供。

`register_imports` 会覆盖同名导入已有处理函数；只影响游戏确实导入的符号。部分注册器先 `import_lookup`，仅补齐空缺。因此全量注册条目不等于最终生效接口数。

启动顺序见 `runtime/boot.c:396` 起：基础 C → SDK → C++ → 线程 → NVN → 文件 → C 库 → 时间 → 输入 → 显示 → 字体 → 视频占位 → 其他补齐 → 音频 → 模块加载 → Media Foundation 视频 → iostream → C/C++ 补齐 → 异常展开 → SDK 第二轮。

第二轮由 `runtime/nnsdk2.c:register_sdk2_imports` 调用：先装载 lifted SDK 和 Mii / applet / album / 网络 / 目录实现，再装内存 override、选择的 SDK 实现和 stdio；最后执行 `nnnet_register_final()`。后者依次执行 c4a、c5a、Yuzu SDK 适配，所以早期看到的网络占位不能直接当成最终实现。是否走虚拟网络还取决于启用状态和 FD 类型。

| 接口族 | 主要实现文件 | 功能 / 注意事项 |
|---|---|---|
| C 内存、字符串、数学、stdio | boot.c、clib.c、nnlibc2.c、nnlibc2_c5b.inc、sdklift_rt.c | 有后续覆盖和原 SDK 接管 |
| C++ 对象、容器、异常、流 | libcxx.c、nncxxrt.c、nncxxlib.c、nniostream.c、nnunwind.c | 异常展开另有 main/NRO 桥 |
| 线程、锁、事件、TLS | nnsdk.c、os_threads.c、nnsvc_c5a.inc | 不要用 nnsdk.c 早期的空锁实现推断最终行为 |
| 文件、存档、目录 | nnfs.c、nnfsdir.c | 本地 userdata 与 RomFS 映射；存档有提交逻辑 |
| 时间、时区 | nntime.c | 时区数据路径与测试固定时钟分别配置 |
| 控制器、触控/输入兼容 | nnhid.c、nnpad_sdl.c | SDL3；8 玩家索引；物理设备能力与源码自测结论分开 |
| 画面、窗口、NVN | display.c、nvn_stub.c、nvn_gl.c、nvn_vk*.c | NVN 名称分发到宿主图形实现 |
| 音频、视频、字体 | nnaudio.c、nnmovie_mf.c、nnpl.c、nnfontll.c | 依赖本地数据/库；视频旧占位有条件被覆盖 |
| 运行时模块 | nnro.c、nro_shim.c | 动态符号、NRO DLL 与替换脚本 |
| Mii、相册、软键盘、小程序 | nnmii.c、nnalbum.c、nnapplet.c | 部分按离线主机行为回答；网页/YouTube 小程序未实际打开浏览器 |
| 账户、好友、BCAT、网络等 | nnnet.c、nnnet_c4a.inc、nnsvc_c5a.inc、nnnet_yuzu.inc | 本地/离线兼容与房间隧道分别看待；官方在线未实现 |
| 新角色与 loose files | fighter_sdk/newmodule_host.c、runtime/loose_files.c、loose_host.c | 清单、资源替换和 Hook 组合 |

## 8. 图形、输入和 DLL 内部边界

NVN 查询链从 `nvnBootstrapLoader` 开始，`nvn_stub.c` 的特殊函数表先处理设备/内存/缓冲区，再交给 `nvn_gl_pick(name)`。其名称虽有 gl，仍是图形前端分发表，后续可转 Vulkan。分发表包含有意无操作的调试/缓存语义项，也包含平台简化；不能一律判作缺陷。

未实现的 NVN 调用默认记日志并停止，`SSBU_NVN_MISSING=warn` 才允许返回 0 继续探索。Vulkan 还有独立的 missing 策略。完整名称与处理函数在 `guest_imports.jsonl` 的 `nvn_dispatch` 类别。

| 头文件 | 内部约定 |
|---|---|
| `nnpad.h` | init/poll/present/get/rumble/disconnect/swap、键盘分配、工具和自测；玩家编号 0～7，摇杆 -32767～32767 |
| `gfx_options.h` | 图形配置结构及加载；当前源码含 0.25～2.0 分辨率倍率 |
| `nvn_front.h`、`nvn_dt.h` | 前端渲染协作、纹理/采样器等内部数据 |
| `nvn_vk_prewarm.h` | Vulkan 预热和流水线准备 |
| `mx_translate.h`、`mx_ops.h` | Maxwell 着色器翻译及操作 |
| `tex_decode.h` | 纹理布局/格式解码 |
| `sdklift.h` | SDK 图像、符号查找、间接调用、override、install 和 setjmp 桥 |
| `eh_runtime.h` | 异常展开的内部数据与声明 |
| `runtime.h`、`neon.h`、`fpexact.h` | 重编译器生成代码使用的内存访问、寄存器和指令辅助 |
| `input_trace.h` | 最终控制器状态的 CSV 观察输出；不是输入回放文件格式 |

DLL 装载约定：`game_bind(GameHost *,GameTables *)` 交换回调与主游戏函数/符号表；`game_eh_bind` 提供异常恢复表；`nro_bind(NroHost *,host_module,NroTables *)` 交换模块函数/符号/异常表；`fighter_bind` 则面向可编辑角色脚本。另有 glslang 桥导出 `ssbu_glsl_to_spirv` / `ssbu_spirv_free`，二进制存在与否取决于构建产品。R1 变体可额外导出脏页记录变量。

## 9. 命令行、环境变量与配置文件

主要命令：

| 命令 | 用途 / 参数 |
|---|---|
| `scripts/setup.py` | 构建；`--dump`、`--build`、`--jobs`、`--modules`、工具路径、`--redo`、`--from`、`--only`、`--status`、`--steps` |
| `scripts/run.py` | 运行；必填 `--userdata`；支持 `--build`、`--seconds`、`--seq`、`--exe`、`--refresh`、`--seed`、`--log`、可重复 `--env`、`--exclude-cpus` |
| `scripts/yuzu_play.py` / `YuzuPlay.bat` | 进程内联机控制窗口；路径、测试存档及日志配置 |
| `YuzuDiagnose.bat` | 联机诊断入口；实际命令见脚本 |
| `scripts/package.py` | 打包；包目录、ROMFS、缓存、存档、配置、ZIP 等选项 |
| `scripts/dumpinfo.py` | 读取诊断转储/本地符号资料 |
| `tools/lifter/build.py` | compile / link / nro；手动解析命令，不是 argparse |
| `fighter_sdk/tools/build_mod.py` | 编译 mod DLL；通过 sys.argv 解析 |
| `fighter_sdk/tools/build_fighter.py`、`new_fighter.py` | 角色脚本构建与新角色目录生成 |
| `tools/package/launch.ps1` 与 BAT | 可分发包的启动、回放、冒烟和诊断入口 |

其他 extract / lifter / nro / fighter 工具及测试入口均按源码路径收入 `cli.jsonl`。手动 `sys.argv` 解析的脚本保留用法和解析代码位置，不把它们假装成已经存在的统一参数规范。

**运行配置陷阱：** `scripts/run.py` 会先清除继承环境里的 `RECOMP_*` 和 `SSBU_*`，然后写入路径和显式参数。因此先在 PowerShell 设置 `$env:SSBU_NET_HOST=...` 再用 run.py，并不能保证该配置传给游戏；应传 `--env SSBU_NET_HOST=...`。

常用环境变量：

| 变量 | 作用 |
|---|---|
| `SSBU_ROMFS`、`SSBU_USERDATA`、`SSBU_FONTS`、`SSBU_TZDIR` | 游戏资源、存档和系统数据路径 |
| `SSBU_NET_HOST`、`SSBU_NET_PORT`、`SSBU_NET_NICKNAME` | 房间服务器、端口、房间昵称 |
| `SSBU_NET_PASSWORD`、`SSBU_NET_TOKEN` | 房间准入参数 |
| `SSBU_NET_LOG_LEVEL`、`SSBU_NET_UI` | 联机日志和控制窗口 |
| `SSBU_NICKNAME` | 初始化游戏内名字；不是房间 nickname |
| `SSBU_MODS`、`SSBU_CHARACTERS` | 模组和新角色目录 |
| `SSBU_RENDERER` | 图形后端；覆盖配置文件 |
| `SSBU_FIXED_SEED`、`SSBU_SEQ`、`SSBU_PLAY`、`SSBU_REC` | 确定性、脚本输入、回放、录制相关入口；格式各自遵循输入代码 |
| `SSBU_PAD_TRACE` | 最终输入观察 CSV |
| `SSBU_SWKBD` | cancel、text:内容、dialog 等软键盘策略 |
| `SSBU_SDK_SELFTEST`、`SSBU_PAD_SELFTEST` | 专用测试入口；不是正常启动参数 |
| `RECOMP_GEN`、`RECOMP_OBJ`、`RECOMP_DLL`、`RECOMP_CLANG` | 构建变体和工具选择 |

完整变量表包含大量历史实验/诊断开关。变量被找到只说明源码有使用位置；默认值、启用条件、是否仅某变体生效，应查同条 evidence 和对应实现。

配置文件边界：

- `config/graphics.ini`：renderer、window、width、height、vsync、latency、runahead、gpu、console_mode、resolution_scale、anisotropic_filtering、shader_wait、shader_prepare、profile、frame_time_overlay。完整可选值和默认值在 `runtime/gfx_options.c` 写出的模板里。
- `config/controllers.ini`：通用设备配置、玩家分配、键盘和按 GUID 的设备覆盖；默认死区 0.10，可热重载；完整字段在 `runtime/nnpad_sdl.c` 的模板和解析器。
- 发行包 `launcher.ini`：由 `tools/package/launch.ps1` 解释；不要将它与上述游戏运行时 INI 混用。
- `fighter.json` 及角色资源清单：由角色工具和 registry/loose-files 实现解释，细节见新角色文档。

## 10. 本次结论能证明到哪里

已核对公共 ABI、主要模块边界、协议报文、导入覆盖顺序、参数校验和错误语义；对全部受版本控制的项目源码作静态索引，并补充本地构建导入/导出。生成记录带位置，可直接回到代码核验。

以下结论不由本次工作提供：所有系统函数与原机行为完全一致、所有注册最终都生效、游戏每个分支都会调用到、真实跨客户端对战成功。需要实际注册转储、调用计数、差分测试或联机实测才能确认。

工作区原有 `runtime/nnnet_yuzu.inc` 与 `runtime/yuzu_ldn.c` 修改已纳入源码阅读，但现有二进制可能早于这些修改；读取导出表只能证明导出存在，不能证明二进制包含工作区最新行为。网页小程序等离线简化和 NVN 有意 no-op 已按源码说明；未自动将统一返回值处理函数判成“全部无效”。

扫描器使用词法匹配而非 C/C++ 编译器，预处理宏展开、C++ 重载运算符及生成包装器以原始头文件/宏/符号表为准。若需要某一系统接口的完整 guest 参数结构，应按对应实现和本地 SDK 符号继续追踪，不能从 `void handler(Ctx *)` 自动还原为已验证的 SDK 函数签名。

本次验证：扫描完整执行成功；检查了宏拼接、字符串内 URL 与注释区分、9 个网络 API 与 EXE 导出的交集、59 条最终网络适配注册、6 个 registry 回调和源码位置；离线检索页通过默认分页、按名搜索、类别筛选及空结果检查。未运行游戏回归或真实联机测试。


## 资源包后端接口（mod-adapt）

`runtime/mod_resources.h` 提供 ABI 1：`ssbu_mods_api_version()`、`ssbu_mods_request(UTF-8 JSON)`、`ssbu_mods_free()`。接口覆盖扫描/加载计划、启停/优先级、配置方案、冲突、兼容能力、准备进度和运行状态；没有新增前端。控制管道接受 `mods<TAB><JSON><换行>` 并返回一行 JSON。

返回值独立分配，必须用同一模块的配套释放函数释放。配置对下一次游戏启动生效，活动资源视图保持不可变。完整操作、字段及例子见 [MOD_RESOURCES.md](MOD_RESOURCES.md)。
