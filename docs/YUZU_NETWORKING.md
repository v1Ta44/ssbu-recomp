# Yuzu 房间互联：后端实现与联调说明

最新验收更新：2026-10-18。用户已确认联机扩展通过跨 Yuzu、PC 客户端之间的双人及三人真实测试，见第 15 节。房间客户端、LDN 服务、虚拟 UDP 套接字与前端预留 C 接口已实现，本机协议集成测试也已通过。联机扩展属于个人 `yuzu` 分支；`main` 保持上游原始副本。以下早期核查和测试记录保留各自当时的验证范围。

目标：本项目作为客户端加入用户现有的 Yuzu 房间服务器，与运行 SSBU 的 Yuzu 玩家互联。用户已确认需要跨客户端对战，服务器程序版本未知。第一阶段采用服务器地址直接加入；房间列表不是实现互联的前置条件。

## 1. 固定的参考版本

- 源码镜像：<https://github.com/yuzu-emu-mirror/yuzu-mainline>
- 提交：`7ffac53c9e67fa18d067114a8eb3e77c74bba68f`
- 提交时间：2024-03-03 13:02:50 UTC。
- 本次查询时镜像 `master` 指向上述提交；这是本项目选定的兼容基线，不据此声称所有 Yuzu 分支、EA 版或后继项目的协议相同。
- 本地只读参考：`deps/yuzu-network-reference/`；`SOURCE.json` 记录获取的文件、提交及 SHA-256。此目录已被现有 `deps/` 忽略规则排除。
- 当前 `deps/suyu/` 的远端实际为 Eden，不作为此次原版 Yuzu 协议基线。
- 参考文件带有 GPL-2.0-or-later 或 GPL-3.0-or-later 标识。若后续移植代码，保留对应文件的版权、许可证与来源说明；具体标识以各文件为准。

停更意味着可以固定源码，不意味着已经知道在用服务器的版本。实现按上述基线进行；服务器握手和真实 Yuzu 对战分别确认房间协议与游戏互操作性。

## 2. 端到端结构

```text
本项目中的 SSBU 本地无线模式                   Yuzu 中的 SSBU 本地无线模式
           │                                             │
    nn::ldn + nn::socket                           LDN 服务 + BSD 服务
           │                                             │
  LDN 状态机 + 虚拟套接字                        LANDiscovery + ProxySocket
           │                                             │
      Yuzu 房间客户端 ───── ENet/UDP 房间服务器 ───── Yuzu RoomMember
```

房间服务器主要根据虚拟 IP 或广播标志转发数据，并不代替游戏模拟对战。兼容层需要接入原游戏已有的本地无线与同步逻辑。

线上 `NexNetworkFactory`、普通 LAN `LanNetworkFactory`、本地无线 `LdnNetworkFactory` 是三种游戏后端。本目标优先走第三种。Yuzu 源码中 `LANDiscovery` 这个类名属于 LDN 服务，不等于 SSBU 的普通 LAN 后端。

## 3. 已核验的房间协议

固定版本的源码依据：

- [room.h](https://github.com/yuzu-emu-mirror/yuzu-mainline/blob/7ffac53c9e67fa18d067114a8eb3e77c74bba68f/src/network/room.h)：版本、消息编号、默认端口。
- [room_member.cpp](https://github.com/yuzu-emu-mirror/yuzu-mainline/blob/7ffac53c9e67fa18d067114a8eb3e77c74bba68f/src/network/room_member.cpp)：加入请求、消息编码和接收处理。
- [room.cpp](https://github.com/yuzu-emu-mirror/yuzu-mainline/blob/7ffac53c9e67fa18d067114a8eb3e77c74bba68f/src/network/room.cpp)：加入校验、虚拟 IP 分配、数据转发。
- [packet.cpp](https://github.com/yuzu-emu-mirror/yuzu-mainline/blob/7ffac53c9e67fa18d067114a8eb3e77c74bba68f/src/network/packet.cpp) 与 [packet.h](https://github.com/yuzu-emu-mirror/yuzu-mainline/blob/7ffac53c9e67fa18d067114a8eb3e77c74bba68f/src/network/packet.h)：整数、字符串、数组与向量序列化。

| 项目 | 基线行为 |
|---|---|
| 房间版本 | `network_version = 1`，为 `u32` |
| 传输 | ENet over UDP；不是直接向 UDP 端口发送自定义裸包 |
| ENet 通道 | 1 个通道，发送到通道 `0` |
| 发送可靠性 | 房间客户端发送队列和服务器转发使用 `ENET_PACKET_FLAG_RELIABLE` |
| 默认端口 | `24872/UDP`；部署可以改端口，不能据此推断用户服务器端口 |
| 多字节整数 | 房间消息中的 `u16/u32/u64` 使用网络字节序，即大端 |
| 布尔值 | 1 字节 |
| 字符串 | `u32` 字节长度，随后是原始字符串字节，不附加结尾零 |
| `vector<u8>` | `u32` 长度，随后是数据字节 |
| `array<u8,4>` | 直接写 4 字节，不附加长度；用于房间 IP 地址 |
| 自动分配 IP | 请求中的首选 IP 为 `FF FF FF FF` |
| 分配范围 | 此版本服务器生成 `192.168.X.Y`，X/Y 各取 1..254，并避免与成员重复 |

关键消息编号及字段顺序：

| 消息 | 编号 | 消息编号之后的字段 |
|---|---|---|
| JoinRequest | 1 | 昵称字符串、首选 IP、版本 `u32`、房间密码字符串、token 字符串 |
| JoinSuccess | 2 | 分配的虚拟 IP |
| RoomInformation | 3 | 房间信息及成员列表；必须完整按上游编码解析 |
| SetGameInfo | 4 | 游戏名字符串、游戏 ID `u64`、版本字符串 |
| ProxyPacket | 5 | 本端地址族 `u8`/IP/端口 `u16`、远端地址族 `u8`/IP/端口 `u16`、协议 `u8`、广播 bool、数据向量 |
| LdnPacket | 6 | LDN 类型 `u8`、本端 IP、远端 IP、广播 bool、数据向量 |
| NameCollision / IpCollision | 8 / 9 | 昵称或地址冲突；转为可读的加入失败原因 |
| VersionMismatch | 10 | 服务器版本 `u32`，应记录期望值和实际值 |
| WrongPassword / CloseRoom / RoomIsFull | 11 / 12 / 13 | 分别处理密码错误、房间关闭、房间已满 |
| HostKicked / HostBanned | 15 / 16 | 退出房间并报告原因 |
| JoinSuccessAsMod | 24 | 同样携带虚拟 IP；客户端可以按成功加入处理，不必使用管理功能 |

服务器先广播 RoomInformation，再发送 JoinSuccess。不能把 ENet 连接建立误认为房间已经加入，也不能漏掉其他成员变化引发的 RoomInformation 更新。

消息里 token 的位置必须保留。该版本存在 `NullBackend`（忽略 token）以及可替换的用户校验后端；不能从协议推断所有部署都需要或都不需要 token。房间密码与 Nintendo 账号认证是不同机制。

### 两种数据消息

`IdLdnPacket` 用于发现、加入和更新本地无线网络。`IdProxyPacket` 用于实际游戏网络数据。两者都经同一个 ENet 通道传输；它们是两种消息类型，不是两个独立的 ENet 通道。

服务器对广播消息发送给除发送者外的成员；单播根据目标虚拟 IP 查找成员并转发。服务器不负责替客户端实现 LDN 状态机或解压游戏数据。

## 4. LDN 兼容点

源码依据：

- [lan_discovery.cpp](https://github.com/yuzu-emu-mirror/yuzu-mainline/blob/7ffac53c9e67fa18d067114a8eb3e77c74bba68f/src/core/hle/service/ldn/lan_discovery.cpp)
- [ldn_types.h](https://github.com/yuzu-emu-mirror/yuzu-mainline/blob/7ffac53c9e67fa18d067114a8eb3e77c74bba68f/src/core/hle/service/ldn/ldn_types.h)
- [user_local_communication_service.cpp](https://github.com/yuzu-emu-mirror/yuzu-mainline/blob/7ffac53c9e67fa18d067114a8eb3e77c74bba68f/src/core/hle/service/ldn/user_local_communication_service.cpp)

| LDN 消息类型 | 值 | 基线负载 |
|---|---|---|
| Scan | 0 | 空，广播查询 |
| ScanResp | 1 | `NetworkInfo`，单播回应 |
| Connect | 2 | `NodeInfo`，发给主机 |
| SyncNetwork | 3 | `NetworkInfo`，主机发给加入者/成员 |
| Disconnect | 4 | `NodeInfo` |
| DestroyNetwork | 5 | 空 |

关键要求：

- Yuzu 用内存拷贝传输 LDN 结构体：`NetworkInfo` 为 `0x480` 字节，`NodeInfo` 为 `0x40` 字节，最大 8 个节点。不能把整个负载按房间消息的大端整数规则重新编码。
- 房间 IP 的四字节顺序与 `NodeInfo.ipv4_address` 不同：`GetNodeInfo()` 将四字节反转后存储，`Connect()` 读取主机地址后再反转。必须明确区分房间地址、结构体字段和 guest socket 地址的表示。
- 虚拟 MAC 由 `02:00` 加房间虚拟 IP 的四个字节生成。
- 状态值依次为 None=0、Initialized=1、AccessPointOpened=2、AccessPointCreated=3、StationOpened=4、StationConnected=5、Error=6。
- 扫描不仅收取回应，还按通信 ID、会话 ID、网络类型、SSID、场景 ID 等过滤。
- 主机收到 Connect 后更新节点列表并发送 SyncNetwork。加入方收到 SyncNetwork 才转为 StationConnected。
- 节点变化需要触发状态事件，否则游戏可能一直等待，即使数据已经收到。
- 退出、销毁网络和房间断线要清理节点并唤醒等待者；避免游戏线程因失去连接永久阻塞。

原项目拦截的是 `nn::ldn` SDK 函数，Yuzu 实现的是其下方的系统服务。不能直接把 Yuzu 的 IPC 参数结构当成游戏 SDK 函数的参数布局；应逐项核对参数顺序、返回结果、输出缓冲大小、字节序和事件对象。

参考版本也有未实现项，例如 `ConnectPrivate` 的服务处理为空、部分策略接口返回成功占位。因此需要用 SSBU 的实际调用记录确定首版必需接口，不能把 Yuzu 的全部行为当作完整平台规范。

## 5. 虚拟套接字兼容点

源码依据：

- [socket_proxy.cpp](https://github.com/yuzu-emu-mirror/yuzu-mainline/blob/7ffac53c9e67fa18d067114a8eb3e77c74bba68f/src/core/internal_network/socket_proxy.cpp)
- [socket_types.h](https://github.com/yuzu-emu-mirror/yuzu-mainline/blob/7ffac53c9e67fa18d067114a8eb3e77c74bba68f/src/common/socket_types.h)
- [bsd.cpp](https://github.com/yuzu-emu-mirror/yuzu-mainline/blob/7ffac53c9e67fa18d067114a8eb3e77c74bba68f/src/core/hle/service/sockets/bsd.cpp)

1. Yuzu 的 BSD 服务在创建 socket 时，如果房间已连接，就创建 `ProxySocket`。本项目也需要明确网络后端选择时机；首版应在进入游戏联机流程前加入房间。
2. `SendTo()` 把原始数据用 Zstandard 压缩后装入 ProxyPacket；接收端解压后放进对应 socket 的队列。LDN 结构体消息没有这一压缩步骤。
3. 接收分发比较协议、目标端口和广播设置。`RecvFrom()` 返回发送者地址和端口，而不是房间服务器地址。
4. 协议内的枚举与 Winsock/guest 常量不同：Yuzu `Domain::INET=1`；`Protocol` 中 Unspecified=0、ICMP=1、TCP=2、UDP=3。不能直接写入 `AF_INET=2` 或 `IPPROTO_UDP=17`。对于创建 socket 时传入的协议 0，应保持与 Yuzu 的翻译结果一致，不擅自全部改成 UDP=3。
5. `INADDR_ANY` 或匹配主机地址的发送端地址会被替换为房间虚拟 IP。这个虚拟 IP 不能直接拿来绑定 Windows 物理网卡。
6. `Poll`、阻塞/非阻塞接收、超时、广播和 Close 必须一起接入虚拟队列。当前项目 `socket_Poll()` 只调用 `WSAPoll()`，单独改 SendTo 不够。
7. `ProxySocket` 对部分 TCP 操作仍有占位实现。先依据 SSBU 的调用轨迹完成实际需要的操作，不据此宣称已经提供通用 TCP 隧道。

## 6. 本项目的具体落点

实际实现文件：

| 位置 | 职责与修改方向 |
|---|---|
| `runtime/yuzu_packet.h` | 有界的大端编码/解码；字符串截断保留 UTF-8 字符边界 |
| `runtime/yuzu_net.c/.h` | ENet 生命周期、异步加入、房间/成员快照、昵称和本地头像接口 |
| `runtime/yuzu_net_internal.h` | 后端共享锁、条件变量和内部接口，不向前端暴露 |
| `runtime/yuzu_ldn.c` | LDN 状态机、发现、建房、加入、节点同步、事件及取消等待 |
| `runtime/yuzu_socket.c` | 虚拟 UDP、Zstandard 封装、端口分发、超时、广播、关闭及断线唤醒 |
| `runtime/nnnet_yuzu.inc` | SDK 适配层；虚拟描述符委托到新后端，原有本机描述符保留原路径 |
| `runtime/nnnet.c` | 最后注册新的 socket/LDN SDK 入口，覆盖原占位实现 |
| `runtime/nnfs.c` | `GetNickname()` 读取可配置的本地账号名 |
| `runtime/boot.c`、`runtime/hostrt.c` | 环境变量启动等待准入，正常退出路径关闭网络线程 |
| `tools/lifter/build_network.py`、`build.py` | 固定 SHA-256 的 ENet/Zstandard 源码、缓存编译与静态链接；现有 stage 自动复制新文件 |

网络线程只更新宿主队列/受锁保护的网络状态、触发事件；不直接在网络回调中运行 guest 游戏代码。避免持有发送队列锁时调用可能再进入队列的回调。

房间客户端的真实网络通信与 guest 虚拟 socket 分开：ENet 自己使用宿主 socket 到达服务器，不经过为游戏提供的 ProxySocket，避免递归代理。

当前源码中网络状态、账号服务仍按离线建模。接入 LDN 不应把所有互联网/账号接口直接改成成功；需要核对本地无线调用的实际网络查询，仅补齐对应的本地网络语义。

游戏侧定位（本地 13.0.4 生成代码）：

- `build/work/gen_eh_o2/c_0495.c` 的 `f_71037046b0` 与 `f_7103704ae0`：按对象 `+0x14` 的值选择网络后端，1=LDN、2=NEX、3=LAN。
- `f_7100167c50`：LDN 工厂构造入口，位于 `c_001d.c`。
- `0x7103704504`：调用 `nn::ldn::Initialize()` 的指令位置。
- `runtime/hostrt.c` 的 import trace 带 `from=` 调用地址，默认仅记录每个 import 的前三次调用。首次调试需要针对网络接口增加有界的专门跟踪，不能把默认日志当成完整时序。

这些定位信息用于分析；后续修改保存在宿主运行时或生成工具中，不手工修改生成的游戏 C 代码。

## 7. 分阶段验收

| 阶段 | 可验收结果 | 不能据此宣称的结果 |
|---|---|---|
| A. 协议编解码 | 用固定上游的序列化输出验证字节一致；覆盖截断、错误长度、未知消息与解压上限 | 尚未连接房间 |
| B. 独立房间客户端 | 在本机参考服务器与用户服务器分别加入/退出，获得成员列表与虚拟 IP，错误密码/重名有明确结果 | 尚未实现游戏发现 |
| C. LDN 互操作 | 本项目和真实 Yuzu 分别作为主机，双方都能扫描、加入并收到节点变化 | 尚未确认战斗数据正常 |
| D. 游戏数据互操作 | 两端传递真实 SSBU 数据，端口、协议、字节序、Zstd、Poll/RecvFrom 均正确 | 尚未确认长时间同步 |
| E. 跨客户端对战 | 同一游戏更新版本和匹配的内容配置，双方各做主机，完成选人、对战、结算、再开一局；记录断线/退出行为 | 未覆盖的角色、模式或多人规模仍需单独验证 |

联调先固定同一游戏更新版本。本项目基于 13.0.4；Yuzu 程序版本与 SSBU 游戏版本是两件事。房间版本号为 1 不代表不同游戏更新、战斗模组或内容配置可以互联。

保留现有离线网络自测；新增一项短时本机集成测试，使用独立编码的 ENet 房间夹具及固定握手字节向量。它检查房间准入、昵称/头像字段、LDN 双向角色、取消旧连接后创建新会话、Zstd 数据收发、异常压缩数据、关闭旧描述符和断线唤醒。夹具没有运行 Yuzu 或游戏同步逻辑，不能替代跨 Yuzu 实测。

```powershell
.\.venv\Scripts\python.exe tests/test_yuzu_backend.py
```

本机执行约 3 秒（包含测试程序编译，已缓存依赖）；测试只绑定 `127.0.0.1` 的临时端口，不连接用户服务器，不依赖游戏、显卡或画面。

本次构建与检查结果：宿主 `ssbu_native.exe` 完整链接成功；新增集成测试全部通过；现有 SDK 自测中的 socket/LDN/账号等网络相关项目通过（close4/A 为 20 项零失败）。全量 SDK 自测返回 4 项失败：`erfcf` 最大 5 ULP 差异，以及 Alberta/British Columbia/Manitoba 三个 Windows 时区的 2026 年转换差异，日志见 `build/logs/yuzu_sdk_selftest.log`。未将此结果记为全量通过，也未改动这些非网络实现。

## 8. 早期待测项与后续状态

- 用户服务器的地址、UDP 端口、是否设房间密码；进入服务器验收阶段时再提供即可，当前源码分析不依赖这些信息。
- 在用服务器是否接受固定的 version=1 消息格式，是否有自定义校验或扩展。
- Yuzu 侧运行同版本 SSBU 时的实际 LDN/socket 调用序列，以及是否触及基线中的未实现接口。
- 跨 Yuzu 与 PC 客户端之间的联机已于 2026-10-18 获用户确认，通过双人及三人真实测试，见第 15 节。原先缺少真实联机验收的状态已更新；本次反馈没有逐项列出选人、结算、重开或断线操作，不补写这些操作的独立结果。

## 9. 用户信息与头像：已经确认的边界

| 信息 | 来源、用途与当前支持 |
|---|---|
| 游戏本地用户 ID | 沿用现有离线账号 `{1,0}`，不需要 Nintendo 服务器登录 |
| 游戏里的用户名 | `nn::account::GetNickname()` → 游戏提供的 `UserConfig.user_name` → LDN `NodeInfo.user_name`；支持最多 32 字节 UTF-8，允许中文。保留游戏传入的名字，不用服务器昵称替换 |
| Yuzu 房间昵称 | JoinRequest 的 nickname，原版服务器要求 4–20 个 ASCII 字母、数字、空格或 `._-`；与游戏名分开配置 |
| 服务器账号资料 | RoomInformation 的 username、display_name、avatar_url；完整解析并以有界副本交给前端，值由服务器校验后端决定 |
| 本地前端头像 | 提供 PNG/JPEG 字节读写 API，最大 256 KiB；只检查文件头/尾特征，不解码图片，前端仍须处理无效图片 |
| 其他玩家的服务器头像 | 保留 avatar_url；本次不下载、不显示图片。未验证身份的部署可能返回空值 |
| 自定义头像上传 | 原版 Yuzu v1 房间协议没有相应消息。本地头像不会自动传给 Yuzu 玩家，也不会改变服务器账号头像 |
| SSBU 游戏内玩家图标 | 当前游戏导入表未发现账号头像读取入口；本后端没有改写游戏自身的图标/存档逻辑，不能把本地头像 API 当成游戏图标同步功能 |

证据：本地 13.0.4 生成代码 `c_001e.c` 的 `0x710016e860` 附近调用账号 `GetNickname` 并复制名字；固定 Yuzu 的 `LANDiscovery::GetNodeInfo()` 复制游戏提供的 `UserConfig`，`RoomMemberImpl::HandleRoomInformationPacket()` 解析服务器头像 URL。LDN 的用户节点结构只有名字等字段，没有头像图片字段。

游戏内名字在 Console 的「开始」页填写，普通游戏、联机与冒烟测试共用，并保存在 `launcher/launcher.ini` 的 `[run] game_name`。旧版 `[net] game_name` 仅在新键不存在时读取；明确清空后不会恢复旧值。名字必填，最多 32 字节 UTF-8，不能全为空白或含控制字符。启动器在启动前传入 `SSBU_NICKNAME`，运行时也会检查；未填写时阻止进入游戏，不再读取 Windows 用户名或生成默认名字。本地头像仍为进程内设置。调用 API 修改名字后，游戏再次读取账号名才会看到新值；应在游戏启动前配置，避免游戏缓存旧名字。

## 10. 给未来前端的接口

头文件：`runtime/yuzu_net.h`。这是线程安全的进程内 C ABI，宿主 EXE 导出这些符号；没有 HTTP 服务或独立控制进程。极简控制窗口在宿主进程中调用这些接口；未来前端也可另行封装进程间通信。接口不包含 guest 指针、ENet 对象或回调。

| 接口 | 用法 |
|---|---|
| `ssbu_net_set_profile/get_profile` | 设置/复制本地游戏用户名；设置时验证 UTF-8 和长度 |
| `ssbu_net_set_avatar/get_avatar` | 复制本地图片字节；format 为 0 清空、1 PNG、2 JPEG；get 返回所需容量，容量不足不复制 |
| `ssbu_net_join` | 配置 size=`sizeof(SsbuNetConfig)`、abi=`SSBU_NET_ABI` 后调用；返回 0 仅表示开始连接 |
| `ssbu_net_snapshot` | 输出结构同样先填 size/abi；得到状态、错误、虚拟 IP、成员及头像 URL、收发/丢包计数的完整副本 |
| `ssbu_net_leave` | 停止网络并等待工作线程结束；通常很快，但系统 DNS 解析可能延长退出时间，前端不要在绘制线程调用 |
| `ssbu_net_set_log_level` | 运行中切换 0–3 级日志，非法值返回 BAD_ARGUMENT；不改变网络状态 |
| `ssbu_net_read_log` | 以游标读取最近 128 行已输出的日志，不回调前端；缓冲至少 1100 字节，慢读取者从尚保留的最早记录继续 |

前端调用顺序：配置账号名/本地头像 → Join → 轮询 Snapshot，等待 `SSBU_NET_JOINED` → 进入游戏的本地无线模式。`CONNECTING` 是传输连接，`JOINING` 是等待服务器准入，均不代表已加入。连接中、已加入或 LDN 活动期间，资料设置返回 BUSY，防止游戏缓存与后端名字不一致。

失败类型区分超时、密码错误、昵称冲突、协议版本、满员、封禁、踢出、断线和异常握手；message 是诊断文本，前端可以根据 error 枚举自行提供中文提示。失败后调用 Leave，并让游戏退出/Finalize 本地无线流程，再重新加入；不做静默重连或继续旧会话。

## 11. 构建、启动与实机验收

正常 link 流程自动下载并校验 ENet 1.3.18、Zstandard 1.5.6；之后复用缓存，没有新增网络运行时 DLL。依赖放在 `deps/`，第三方声明见 `docs/NETWORK_THIRD_PARTY.md`。仅重建宿主，无需重新提升/编译游戏代码。

无前端时可用环境变量启动。仓库的 `scripts/run.py` 会清除继承的 `SSBU_*`，所以通过它启动要使用 `--env` 明确传入，并指定存档目录：

```powershell
.\.venv\Scripts\python.exe scripts/run.py --userdata D:/pcsmash/userdata-yuzu `
  --env SSBU_NET_HOST=你的房间服务器地址 `
  --env SSBU_NET_PORT=24872 `
  --env SSBU_NET_NICKNAME=PCPlayer `
  --env SSBU_NICKNAME=小明
```

端口改为实际 UDP 端口；有密码时再传入 `SSBU_NET_PASSWORD`，需要身份校验时再传入 `SSBU_NET_TOKEN`。直接运行宿主 EXE 则可以使用普通环境变量。

环境变量启动在 guest 网络调用前最多等待 12 秒准入，记录成功或错误；不会把传输连接建立误当作加入成功。未设置 `SSBU_NET_HOST` 时维持离线模式。不要在要分享的日志/配置截图中写入 token；后端本身不记录密码或 token。

实机按顺序检查：两端均为 SSBU 13.0.4 且内容/影响战斗的模组一致 → 加入同一个 Yuzu 服务器房间 → 两端进入**本地无线**模式 → 分别让本项目和 Yuzu 创建游戏房间 → 检查房间名字、加入、离开 → 选人、完整对战、结算、再开一局 → 断开服务器时可以退出并重新建房。服务器房间列表和 SSBU 游戏房间列表是两层，不要混为一谈。

明确限制：只提供 IPv4 UDP 游戏数据代理，TCP/IPv6 返回不支持；不连接 Nintendo 官方在线服务；无公共大厅发现、聊天或房间管理界面。LDN 扫描最多缓存 64 个网络、每个网络最多 8 个节点；接受策略支持 AcceptAll/RejectAll，黑白名单不伪装成功；Private 入口按会话 ID 发现/连接，不宣称实现了完整原机安全策略。发送队列（含 ENet 待确认数据）及总接收负载各有 8 MiB 上限，接收另有每套接字字节数/包数上限；单个解压后 UDP 数据报最多 65507 字节。

## 12. 联机调试日志

日志统一以 `[yuzu-net ...]` 开头，写入 stderr。每行包含 UTC 时间、启动日志以来的毫秒数、进程/线程 ID、递增序号、级别和模块，便于对照两端记录。通过现有启动器的 `--log` 保存 stdout/stderr 到同一文件，不增加前端或独立日志服务。

| `SSBU_NET_LOG_LEVEL` | 内容 |
|---|---|
| `0` | 关闭联机日志 |
| `1`（默认） | 服务器地址/端口、协议基线、连接与准入状态、虚拟 IP、LDN 状态、加入开始/结束、断线原因及会话包数 |
| `2` | 加上成员资料是否存在、扫描结果、SDK LDN 调用结果与 guest 返回地址、套接字创建/绑定/选项/错误、解压和队列异常；每 5 秒记录收发/丢弃包数、排队字节、ENet RTT 和丢包计数 |
| `3` | 加上消息编号、LDN 类型、虚拟端点/端口、协议、压缩前后字节数、分发数量与收发返回值；不输出正文 |

`2`、`3` 级各最多输出每秒 64 行，省略条数累计显示在 `suppressed debug=... trace=...` 中；限流不影响 `1` 级的关键状态记录。计数是本进程累计值，`epoch` 用于区分连接会话，`generation` 用于区分 LDN 生命周期。常见的非阻塞 EAGAIN（errno 11）只在 `3` 级记录，避免正常轮询淹没错误信息。详细日志采用同步输出，仅在调试时开启；输出到很慢的终端或磁盘仍可能影响时序。

实机联调建议先用 `2` 级，定位到数据收发阶段再短时改用 `3` 级：

```powershell
.\.venv\Scripts\python.exe scripts/run.py --userdata D:/pcsmash/userdata-yuzu `
  --env SSBU_NET_HOST=你的房间服务器地址 `
  --env SSBU_NET_PORT=24872 --env SSBU_NET_NICKNAME=PCPlayer `
  --env SSBU_NET_LOG_LEVEL=2 --log D:/pcsmash/build/logs/yuzu-game.log
```

日志不写密码、token、昵称正文、头像 URL/图片或游戏数据内容；保留 IP/端口和调用地址以便排查路由。传输失败会标出 DNS、ENet 建连或发送等具体阶段；OS 错误码可能未被底层设置，因此同时保留阶段描述。集成测试复用同一次约 3 秒的运行检查日志分级、限流和敏感字段不输出，示例记录保存在 `build/logs/yuzu_backend_test.log`。

## 13. 极简实测控制窗口

双击仓库根目录的 `YuzuPlay.bat`，或者运行：

```powershell
.\.venv\Scripts\python.exe scripts/yuzu_play.py --nickname 小明
```

启动前通过 `--nickname` 明确填写游戏内名字，或使用当前构建目录 `launcher/launcher.ini` 中保存的名字；留空会阻止启动。不改头像。服务器房间昵称默认仍为 `vitas`，重名时可单独修改；它与游戏内名字分开。输入服务器地址、UDP 端口和所需密码；Token 不需要时留空。点击“连接”，看到“已加入服务器房间”及虚拟 IP 后，点击“启动游戏”。游戏内选择本地无线模式，与同服务器的 Yuzu 玩家发现/加入游戏房间。

窗口在游戏进程内运行，连接和断开在工作线程上处理，启动游戏按钮会等待服务器准入成功。游戏启动后仍可查看成员/日志和控制连接。重连前先在游戏里退出本地无线会话；窗口不会强行修改游戏的会话状态。启动游戏前关闭控制窗口会取消本次运行；启动游戏后关闭控制窗口会保留当前游戏和网络连接，关闭游戏时统一清理。

日志默认开启调试级别，完整游戏日志保存在 `build/logs/yuzu-play-日期时间.log`，可用“打开日志文件”按钮查看。窗口显示近期联机日志，切换日志级别即时生效。密码和 Token 不写配置文件。

默认使用 `build/userdata_yuzu` 作为实测存档；首次运行时从仓库 `userdata` 复制，之后继续使用该测试副本。也可通过 `--userdata 目录` 指定已有测试存档。不启用固定随机数或确定性测试覆盖，保持游戏自己的联机同步流程。

实现：`runtime/yuzu_ui.c`（Win32 窗口）、`scripts/yuzu_play.py`（路径、存档、日志启动配置）。没有独立模拟后端，控制的就是运行 SSBU 的同一个网络实例。

2026-10-15 首次实测记录：用户通过控制窗口成功加入现有服务器，房间显示 2 名成员，ENet RTT 约 27 ms，随后启动了游戏。观察时 LDN 尚未初始化，因此这里只确认服务器准入/成员信息与启动链路，尚未确认跨客户端游戏房间发现和对战同步。

## 14. Console 的联机操作

Console 统一从「开始」页普通启动游戏，不再提供单独的联机启动选项，也不等待服务器连接。游戏启动后，用户可以在联机页自行连接 Yuzu 服务器；连接成功后再进入游戏的本地无线模式。不连接服务器仍可正常游玩。未启动时，联机页引导返回开始页，不自动启动或连接；冒烟测试期间不提供连接操作。

本次仅调整页面流程，复用现有普通启动和运行中 `net_join` / `net_leave` / `net_poll` 接口。后端、原生运行时、游戏文件与原有命令行调试窗口均未修改。断开或重连前仍须先在游戏里退出本地无线会话。

## 15. 用户确认真实联机测试通过（2026-10-18）

用户在项目会话中明确反馈：“联机扩展已经通过了跨yuzu，同pc端的双人及三人真实测试。”

据此，当前验收状态更新为：

- 跨 Yuzu 客户端联机：真实测试通过。
- 本项目 PC 客户端之间联机：真实测试通过。
- 已确认的参与人数：双人、三人。

结论来自用户完成的真实测试，与前文协议夹具、回环和离线检查分别记录；不再将联机扩展整体描述为仅进房成功或跨客户端实测待完成。记录日期为用户确认日期，不推定每次测试的具体日期。

本次反馈未附可对应的日志、客户端版本、房主分配、时长、网络条件及逐项操作结果，因此不补写这些细节，也不将双人、三人结果扩展为更多人数、全部客户端版本、全部模式或长期稳定性均已验收。该反馈确认联机扩展的实测状态，不新增延迟滑块的端到端量化指标。
