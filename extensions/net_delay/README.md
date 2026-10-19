# Net Delay module

沿用用户于 2026-10-18 实测确认的输入等待帧调整路线。核心、游戏适配、进程通信和 Console 风格界面分开实现。

- `include/delay_api.h`：与游戏、网络、窗口和渲染器无关的 C 数据接口。
- `core/delay_engine.cpp`：选择值、应用条件、会话身份与统计。只依赖 C++ 标准库。
- `adapter/ssbu_1304.cpp`：唯一接触游戏地址、HookApi 和 Yuzu 状态的适配文件。
- `adapter/preferences.*`：设置的读取、校验和原子替换保存。
- `adapter/bridge.*`：将控制接口提供给本机独立前端。
- `ui/win32_panel.cpp`：通用窗口/WebView2 容器，不接触游戏内存或网络后端。
- `ui/web/*`：界面布局、交互和文案。样式和字体直接取自本仓库 `tools/console/ui`，不复制维护第二套设计。

面板默认完全隐藏，游戏前台按 F8 唤出，再次按 F8 隐藏；隐藏时没有状态条或点击区域。

核心可独立构造多个实例；调用 `nd_evaluate()` 时由适配层提供前后会话、原函数结果和一次性的写入回调。前端可以独立替换或关闭，后端不依赖其存在。

构建、使用、验证结果与限制见 [模块说明](../../docs/NETWORK_LATENCY_MODULE.md)。旧实验基准保存在 Git 标签 `net-delay-verified-20261018`。
