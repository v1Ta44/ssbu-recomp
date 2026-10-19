# 减延迟模块：已验证路线的解耦实现

日期：2026-10-18。功能已合入 `yuzu`；原开发分支：`codex/yuzu-latency-slider`。

用户实测生效的路线已固定到 Git 标签 `net-delay-verified-20261018`（指向 `3e5c3a5`）。新版继续在原游戏计算完成后调整本机等待帧数，没有改变游戏速度、渲染帧索引或玩家之间的网络报文。

本次将旧的单文件实验重写为独立后端、游戏适配层和独立前端，改用贴在游戏窗口右上角的 Console 风格面板。原生下拉框窗口已移除。

## 使用

仍然双击独立工作目录中的 `LatencySlider.bat`：

```text
C:\Users\haoye\.codex\worktrees\yuzu-latency-slider\pcsmash\LatencySlider.bat
```

通过原有联机面板连接服务器并启动游戏后，面板默认完全隐藏。游戏处于前台时按 **F8**，在画面右上角展开设置；点击“收起”、面板获得焦点时按 **Esc** 或再次按 **F8** 隐藏。隐藏状态没有状态条、边框、按钮或鼠标点击区域。

- **自动**：保留游戏给出的等待帧数。
- **手动**：用加减按钮或滑杆选择 0～25 帧。第一次切到手动时，已知原值会用作起点；没有记录时起点为 3 帧。
- **已选择，等待应用**：选择已经保存，但游戏还没有在当前会话中应用。
- **已应用 N 帧**：本次会话中的实际结果与当前选择一致。
- **恢复自动**：停止后续手动覆盖。它不承诺重新计算旧值；需要干净对照时重新进入本地无线会话。

手动设置只在本地无线会话已有其他玩家接入时应用。收起面板不会重置设置，也不会结束游戏。修改后自动保存，下次启动继续使用；首次没有保存值时为 Auto。

设置保存在实验专用 `build/latency-slider/config/online-delay.ini`，不写入游戏存档。`--frames auto` 或 `--frames 3` 可临时覆盖本次启动的初始设置；只有操作面板或设置 API 时才保存新的选择。

面板是由独立进程承载、绑定到游戏窗口的悬浮窗口，支持当前项目的窗口和无边框显示；它没有注入 Vulkan/OpenGL 绘制或改写游戏 UI 资源。键鼠操作已实现；本轮未增加手柄导航或独占全屏支持。操作面板不用于测量按键到屏幕的实际延迟。

## 前端严格复用 Console

构建时直接读取本分支的：

```text
tools/console/ui/style.css
tools/console/ui/fonts/*
```

原样复制到生成目录，不单独维护配色、字体或动画副本。保留 Console 的红金主色、斜切板块、Smiley Sans/Saira、深浅主题、背景条纹、入场、扫屏、选项弹跳、按钮反馈、状态菱形和减少动态效果设置。`panel.css` 只处理面板布局和窄窗口内容，不重写主题色或动画曲线。

不执行 Console 的 `app.js`，也不依赖 Tauri 启动器状态。WebView2 容器只加载本地面板页面；界面通过独立的数据协议读取状态和提交选择。

## 分层与集成边界

```text
Console 风格页面
        │ 选择 / 读取
独立 delay_panel.exe
        │ 本机控制协议
游戏进程中的 bridge 适配
        │ C 数据接口
独立 delay_engine
        │ 一次性写入回调
ssbu_1304 适配 ── 原游戏计算函数 / 原有 LDN 只读快照
```

| 层 | 位置 | 依赖范围 |
|---|---|---|
| 核心 | `extensions/net_delay/core` | 仅 C++ 标准库；没有 Windows、游戏、网络和渲染器头文件 |
| 核心接口 | `extensions/net_delay/include/delay_api.h` | 固定大小 C 记录、函数和不透明实例 |
| 游戏适配 | `extensions/net_delay/adapter/ssbu_1304.cpp` | 目标指纹、HookApi、游戏对象访问和 LDN 上下文映射 |
| 设置与通信适配 | `extensions/net_delay/adapter` | Windows 文件与命名管道；不属于核心判定逻辑 |
| 前端容器 | `extensions/net_delay/ui/win32_panel.cpp` | Windows 窗口、WebView2、控制协议；不含游戏地址或 Yuzu API |
| 前端页面 | `extensions/net_delay/ui/web` | 状态显示、选择交互及 Console 设计复用 |

后端可在没有前端时工作。新的界面只需实现查询和选择，不需要了解游戏寄存器或内存。替换网络后端时，仅调整游戏适配提供的通用会话记录。

原项目运行时继续只提供实验阶段已有的小型只读 `ssbu_net_ldn_snapshot()`；本次重写没有扩大渲染、输入和网络报文修改范围，没有改动 Console 原资产、帧索引任务的文件或原构建入口。

`mods/latency_slider/control.h` 和旧导出函数保留为兼容查询入口，便于原有计算对照测试继续执行；当前界面使用新的 `NdSnapshot`，不依赖旧实验窗口或其布局。

## 原路线保持什么

- 13.0.4 的函数入口仍为 `0x71016ccae0`，参数仍是输入对象的第 65 字节。
- 启动前核对同一段 400 字节代码的 SHA-256；不匹配则拒绝安装 hook。
- 原函数总是先执行；Auto 不写游戏状态。
- 手动值范围仍是 0～25；需要已有其他玩家的本地无线会话。
- 比较计算前后的连接与会话身份，写入前再检查一次当前上下文。
- 不保留旧游戏对象指针给界面定时写入；每次只操作本次函数调用中的对象。
- 界面区分选择版本与应用版本，且核对最后应用属于当前会话，避免把旧结果误标为当前生效。

核心可创建多个独立实例。所有状态在实例内部同步；写入回调在本次判定内同步执行，禁止从回调重入核心。

## 构建与运行

仍使用独立目录，读取 `D:/pcsmash` 的已有游戏生成结果。原游戏、宿主和 ELF 的哈希在构建前后核对；不重编译或覆盖游戏 DLL。

```powershell
python scripts/build_latency_slider.py --base D:/pcsmash
python scripts/play_latency_slider.py
python scripts/play_latency_slider.py --frames auto
python scripts/analyze_latency_slider.py
```

宿主继续使用原项目固定的 clang。独立模块使用本机 MSVC Build Tools，边界只交换 C 记录。前端采用 WebView2；构建器从 Microsoft NuGet 固定 SDK `1.0.3650.58`，校验 SHA-256：

```text
911a472128c82ac8baa0c486c23342cc9dd6e7dc50d754e676726642ca065c60
```

SDK 在忽略目录 `deps/webview2`；生成目录附带 Microsoft SDK 许可和 Console 字体许可。构建时需要网络下载的仅是 SDK 首次取得；后续重用已校验包。运行使用机器已有的 Evergreen WebView2，不在游戏逻辑中创建浏览器。

生成结构：

```text
build/latency-slider/
  host/                       独立宿主和游戏模块副本
  host/plugins/latency_slider.dll   核心与游戏适配
  host/net_delay/delay_panel.exe    独立前端
  host/net_delay/ui/                页面与原样 Console 样式/字体
  config/online-delay.ini      保存的选择
  runs/<运行时间>/            日志、参数记录和界面缓存
  previews/                   自动生成的真实前端截图
  build.json                  构建输入、产物、模块、页面及样式哈希
```

旧实验无需保留运行。退出旧程序后用同一个 `LatencySlider.bat` 即可加载新版。

## 已执行验证

1. 原函数对照：**2,125 个计算/会话条件用例**通过，手动写入仅改变目标字节，原寄存器、标志和栈保持一致；不支持的版本、缺少快照接口等路径继续拒绝修改。
2. 独立核心：**1,088 个判定用例**通过，并检查实例隔离、选择版本、同值选择不增加版本、16,000 次并发选择/快照。
3. 设置：27 档值保存再读取、非法内容、损坏文件和无效保存检查通过。实际插件跨进程重启后恢复 3 帧，临时 Auto 覆盖不改写已保存的 3 帧。
4. 前端：真实 WebView2 DOM 驱动自动/手动、加减、恢复自动和滑杆操作；滑杆编辑期间收到刷新不会复位。
5. 通信：真实页面通过独立前端进程和私有管道更新真实后端，再返回页面显示；没有用另一份模拟界面代替。
6. 窗口：使用离屏测试窗口检查明确的 owner 绑定、移动/缩放跟随、客户区内定位、隐藏状态创建后展开、连续三次展开/隐藏、原生窗口与内部网页可见性同步、owner 隐藏/恢复和关闭后的前端退出。隐藏时原生窗口和 WebView 均不可见。测试不覆盖用户桌面。
7. 加载失败：缺少页面资产时明确返回失败并写入 `frontend.log`，不以进程存活代替加载成功。
8. 设计：构建资产与本分支 Console 的 CSS、字体和许可逐字节一致；检查深色、浅色和等待状态的真实截图，并捕获真实进程从隐藏状态展开后的页面。收起状态完全隐藏，不生成状态条预览。

复现：

```powershell
python tests/test_latency_slider.py --base D:/pcsmash
python tests/test_net_delay_module.py
python scripts/play_latency_slider.py --smoke-seconds 25 --overlay-smoke
```

原实验已经由用户确认实际生效。重写版本通过上述对照和集成检查；黑屏与完全隐藏修复提交 `2d38f34` 后，用户于 2026-10-18 明确反馈“测试通过”，并要求作为新增功能合入 `yuzu`。该反馈记录为修复版本的实际使用验收，不推导新的端到端延迟降幅或长期双端稳定性结论。

## 2026-10-18：黑屏与隐藏行为修复

用户反馈实际窗口全黑后，排查记录确认：WebView2 控制器在隐藏的父窗口中创建，初始 `IsVisible=FALSE`；旧代码只显示 HWND，没有同步控制器可见性。原有页面截图和隐藏进程通信检查漏掉了这项状态差异。

现在统一管理 HWND 与控制器的显示状态，展开时明确打开网页显示，隐藏时同步关闭；窗口移动/缩放后同步位置和网页边界。启动默认隐藏，F8 唤出，原收起状态条已删除。

加载期间提供原生文字提示；加载失败、网页进程退出或超过 20 秒未就绪时显示错误文字，具体状态写入本次运行 `interface-cache/frontend.log`。后端设置继续独立运行。

回归检查使用与正常启动一致的隐藏创建路径，实际检查 HWND 与控制器可见性，重复展开/隐藏，并检查 owner 隐藏和恢复后的状态及展开后的页面捕获；不再仅以通信成功判定显示成功。

## 合入 yuzu（2026-10-18）

用户验收通过后，通过合并提交 `342371a` 将功能分支合入 `yuzu`，保留原开发提交与实验标签。合并没有冲突，功能代码与已验收的 `2d38f34` 相同。

继续保留独立模块目录、C 接口及 `LatencySlider.bat` 入口；普通启动入口的默认行为未调整。上述实验工作目录中已有构建包可以继续使用；在其他 `yuzu` 工作目录使用时，先执行本文的构建命令生成本目录的运行包。

游戏帧索引和本机等待帧数属于不同控制项，不共用开关、不推算相加收益。此次合并不包含帧索引并行分支，也不包含 `D:/pcsmash` 的未提交渲染修改、测试或调查文档。旧调查与用户确认记录仍保存在 `docs/NETWORK_LATENCY_SLIDER.md` 和上述 Git 标签。


## yuzu 个人分支的正常构建入口（2026-10-18）

日常构建可直接使用 `python scripts/setup.py --only link --with-latency-slider`，无需另建实验宿主。
`--without-latency-slider` 为默认关闭项；与资源 MOD 的 `--with-mods` / `--without-mods` 完全独立。
启用后宿主自动加载自身目录下的插件，关闭后不加载残留的延迟插件。打包只包含当前构建启用的功能。
详细组合、依赖与配置位置见 [BUILDING.md](../BUILDING.md#optional-extensions-on-yuzu)。

原隔离实验构建入口保留；其插件与前端现在统一放在实验 `host/plugins/` 和 `host/net_delay/` 下，
启动脚本根据构建清单定位插件。只构建插件的 `--plugin-only` 和原生独立测试仍使用各自测试目录。
