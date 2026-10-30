# 原生科技适配工具

这是 Tech Tree & Research Planner 附带可选工具的开发测试包。
玩家无需安装 Python、Rust 或编译器。

- INSTALL-TECH-ADAPTER：根据已选来源 Mod 生成游戏内规划器，校验备份后
  更新本规划器自己的科技树包。运行前关闭群星和启动器；不修改来源 Mod。
- OPEN-TECH-TREE：打开只读浏览器科技资料。目前公开工坊的浏览器入口仅限
  Windows，不同步存档进度或游戏内目标。PDF 导出保留中英文名称与 ID 文字层。

使用说明见 launchers/ADAPTER-README.txt 和 launchers/WEB-ATLAS-README.txt。
运行根目录脚本，核对检测到的路径和来源后按回车；来源或规划器更新后需重跑。
支持标准科技定义，特殊脚本及解锁条件仍由游戏判断；并非游戏内实时发现。
不注入 DLL，不上传游戏数据。作者自有代码按 MIT 开源，第三方资料保留原许可。
程序与启动脚本测试不等于游戏界面或浏览器界面已验收。
