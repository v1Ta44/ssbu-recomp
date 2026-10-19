ssbu-recomp 测试包
==================

本包只用于在自己的电脑上测试。游戏数据 (romfs\data.arc 等) 来自你自己的游戏备份，请勿外传。

运行要求: Windows 10/11 64 位, 支持 Vulkan 1.3 的显卡驱动。VC++ 运行库已随包附带 (game 文件夹)。
播放游戏的动画需要 Windows 自带的 "VP9 视频扩展" 和 "Web 媒体扩展" (Microsoft Store, 大多数电脑已预装);
控制台会检查并提示。Windows N 版需要先安装 "媒体功能包"。
请复制整个包: romfs 文件夹里除了 data.arc 还有 .nrr 文件夹 (隐藏名称以点开头), 缺了它游戏会卡在对战前的加载画面。

怎么用
------
  首次启动前，在控制台「开始」页填写「游戏内名字」；普通游戏、联机与冒烟测试共用。
  名字必填，最多 32 字节 (约 10 个常见汉字)，会自动保存；不会读取 Windows 用户名。
  双击 SSBU Console.exe (控制台)。需要 Windows 自带的 WebView2 运行时, Windows 10/11 一般已经装好。
    开始    选择 游戏 / 联机 / 冒烟测试, 点大按钮启动。"详情" 里: 启动前检查, data.arc 所在的 RomFS 文件夹和存档文件夹,
            最近几次运行和报告, 开始页的背景图 (例如你自己那份游戏的封面; 控制台不自带图片, 选好后存在 config\cover.*)。
            冒烟测试: 游戏自己运行 90 秒 (片头动画 -> 标题 -> 演示画面), 期间请不要操作; 每 20 秒截一张图放进报告。
            显示"通过"说明没有崩溃、卡死、显卡设备丢失, 片头动画正常开始, 完整运行了 90 秒。使用存档的副本。
    画面    渲染器、窗口、垂直同步、输入延迟、内部分辨率、低配预设。改动立即保存, 下次启动游戏时生效。
    控制器  手柄图上点一个键, 再按手柄或键盘上的键即可绑定; 逐键录入、按设备单独设置、玩家槽位、死区和震动。
            改动立即保存, 运行中的游戏一秒内读取。打开这一页时控制台会检测手柄 (游戏运行时不检测)。
    模组    自动检测本包是否支持资源模组；查看替换内容，设置别名、启停和服装槽位，导入 ZIP／文件夹，
            打开目录或删除到模组回收区。关闭游戏后可修改。
    联机    游戏照常从开始页启动；之后按需在联机页连接服务器，再在游戏里选"本地无线"。
    调试    游戏日志、写 minidump (游戏卡住时先写 dump 再结束游戏)、下次启动的测试选项、运行记录选项。
  Replay.bat      把报告 zip 拖到它上面: 用报告里的存档、设置、随机种子和手柄输入录像重新运行一次 (开发者用)。

出错时
------
  控制台会显示结果 (出错 / 未通过) 并请你写一句说明出错前在做什么, 然后把
      reports\<时间>_<电脑名>_<模式>.zip
  发给开发者。每次运行都会生成 zip (调试页 -> 报告 zip 可以改为只在出错时生成)。
  请不要直接双击 game\ssbu_native.exe: 那样没有日志, 存档和缓存的位置也不对。

报告里有什么
------------
  game.log          游戏的全部输出
  session.json      build 号、设置、传给游戏的环境变量、退出码、结果
  system.txt        系统、CPU、显卡和驱动版本、Vulkan、显示器、手柄
  preflight.txt     启动前检查 (包是否完整、data.arc、Vulkan、磁盘空间)
  input.rec         1 号玩家的手柄输入录像 (回放用)
  save_before.zip   本次运行开始时的存档 (回放用)
  config_before\    本次运行开始时的设置 (graphics.ini, controllers.ini)
  fight_hash.txt    每个战斗帧的角色状态 (回放时逐帧比对)
  boot_trace.txt    启动阶段记录
  ssbu_native_crash.txt / ssbu_native_freeze.txt / *.dmp   崩溃或卡死报告和 minidump (发生时才有)
  events.txt        本次运行期间的 Windows 错误事件 (显卡驱动重置、硬件错误等)
  note.txt          你输入的说明

设置
----
  都可以在控制台里改。文件本身: launcher\launcher.ini (路径、运行记录选项、联机地址; 不保存密码),
  config\graphics.ini, config\controllers.ini (游戏自己的画面和手柄设置, 第一次运行时自动生成)。

资源模组（仅构建时启用的包）
--------------------------
  是否启用记录在 manifest.json 的 features.resource_mods。
  启用包附有 mods\README.txt：每个模组放在包根目录的 mods\<模组名>\ 下，
  例如 mods\MySkin\fighter\...；mods 与 game 同级。下载包的 ultimate/mods/MySkin 外层需取出 MySkin。
  不要放进 game/mods 或 romfs，不需要修改游戏文件。模组改变后重新启动游戏。
  打包不会自动携带开发机安装的第三方模组。Switch 的 .nro 插件不能直接运行。
  关闭版不含资源后端；仅复制 DLL 或模组进去不会启用，需要重新构建启用版。

文件夹
------
  game\        游戏程序 (exe, DLL, 脚本模块)        binaries\  游戏主程序数据
  romfs\       data.arc (如果打包时包含)             sysdata\   字体
  cache\       着色器缓存 (第一次运行时生成, 约 3-4 GB; 第一次运行前 15 分钟左右可能卡顿)
  userdata\    存档                                  logs\      每次运行的日志文件夹 (保留最近 30 次)
  reports\     报告 zip                              manifest.json  本包的 build 号和每个文件的校验值
