# Stellaris Tech Tools

Open-source external tools bundled with **Tech Tree & Research Planner**:
[Steam Workshop](https://steamcommunity.com/sharedfiles/filedetails/?id=3818261596).

| Workshop1.3.1 tools | Source |
| --- | --- |
| Windows, Linux, Intel Mac and Apple Silicon in-game adapter | `tools-1.3.1` tag |
| Windows local browser tree and searchable PDF export | `tools-1.3.1` tag + optional display restoration below |

The Workshop already includes the programs. Players do not need Python, Rust,
this repository or a separate download. This repository publishes the external
programs and launchers, not the private authoring repository or its history.
MIT applies to our code; third-party notices keep their original licenses.

## What the tools do

- `INSTALL-TECH-ADAPTER`: reads game data and enabled source Mods from the
  selected playset, generates a native planner, backs up and atomically replaces
  only this Workshop Mod's owned `tte_runtime.zip`. Close game/launcher first.
  It keeps the same Workshop identity. It does not enable Mods or edit playsets.
- `OPEN-TECH-TREE.bat` (Windows): reads the selected sources and serves a zoomable,
  searchable tech tree on `127.0.0.1`. It does not read save progress or change
  in-game goals. Use Export PDF → Save as PDF for an offline copy with real
  bilingual names and technology IDs. Close its terminal to stop the local server.
- No DLL injection, background service, account credentials or automatic
  download of game/Mod files. No elevation is requested. Reports/backups stay
  in `~/TTE-Research-Planner`; reports contain local source paths.
- Standard directory/ZIP technology scripts, overrides, variables, inline
  scripts and prerequisites are supported. Unusual/dynamic conditions can be
  unknown or unsupported; the game remains responsible for research draws.

## Verify the distributed programs

`workshop-1.3.1/SHA256SUMS` lists the exact executable, launcher and notice files
included in the frozen Workshop1.3.1 tools. You can use your own SHA-256 tool, or optionally:

```text
python tools/verify_workshop.py PATH_TO_3818261596
```

This developer/auditor helper only reads named files. It does not run programs.
Python is not a player dependency. The game ZIP is outside this source scope.

## Build and test

Author toolchain used for the shipped programs: Rust1.98.0, locked Cargo
dependencies. Windows additionally needs the MSVC C++ build tools; Linux's
standalone release uses the musl target. Mac releases are built/signed on Mac.

```text
cd native_mod/compiled_adapter
cargo test --locked
cargo build --release --locked
```

Run from that directory so `.cargo/config.toml` applies Windows static-CRT
linking. Sources include authored fixtures for unknown IDs, changed parents,
overrides, localization, five goals, advice, repeatables, safe writes and launchers.
They do not require a user's game, saves or account. Fixture tests are not game QA.

1.3.1 adds five explicit in-game goal slots and confirmed replacement/swap.
Windows game UI was checked with vanilla/UIOD/Dark Blue, including2.0×. All
four native platforms passed wrapper fixtures. Mac/Linux game UI and Chrome
PDF Find UI are not newly accepted; see `workshop-1.3.1/RELEASE.json`.
Previous releases remain at `adapter-1.2.0` and `viewer-1.3.0`.

The default public browser build uses authored field icons and no bundled
legacy search-hint snapshot. Copyrighted game artwork is not redistributed here.
For the exact display assets used by the shipped Windows viewer, optionally run:

```text
python tools/restore_display_assets.py PATH_TO_WORKSHOP_VIEWER_EXE --restore
```

That explicit developer step reads our precisely SHA-checked executable without
executing it, verifies both asset hashes, and writes only those two assets into
this checkout. It does not scan drives, access process memory or alter the game.
The Rust source stays identical. Without it, source-based technology collection
and planning still work, with field placeholders and reduced legacy pinyin hints.
No bit-for-bit build reproducibility is claimed across compilers/platforms.

## 中文

本仓库公开随工坊分发的外部程序与启动脚本。普通玩家仍可直接使用工坊附带
的程序，不用安装 Python、Rust 或另行下载。本仓库不含私人开发仓库历史、
存档、账号配置或其他 Mod 的原始文件。

适配工具读取当前播放集，备份后只替换本规划器的科技脚本包；浏览器入口仅在
本机回环地址提供只读科技资料，不同步存档进度与游戏内目标。源码、构建说明、
随包文件的 SHA-256 与只读校验工具均在仓库内。1.3.1 可在图中直接设置五个目标，
浏览器可导出含中英文名称和 ID 文字层的 PDF。默认源码构建使用学科占位图；
有游戏美术与旧拼音提示数据的原版展示资源可从已订阅的精确版本程序中本地
恢复，恢复脚本不执行该程序、不扫描硬盘、不接触游戏内存。
