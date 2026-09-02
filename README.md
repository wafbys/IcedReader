# IcedReader

Windows 优先的桌面电子书阅读器。产品名沿用 IcedReader，技术栈是 **Tauri 2 + React + Rust**，正文用系统 WebView 渲染 EPUB 的 HTML。

当前目标平台顺序：Windows → macOS / Linux → iOS → 鸿蒙。第一版只出品 EPUB，格式层按可扩展接口来写。

## 现在能做什么

- 打开本地 `.epub`（EPUB 2 / 3）
- 显示书名、作者、当前章名
- 分页阅读（Foliate / Epub.js 那套 CSS 分栏）：每栏正文最多约 720px，窗口够宽且横屏时双栏，多出的宽度当页边；左右键 / 点左右侧 / 滚轮翻页，章边界再进上一章或下一章。目录用锚点标在同一文件里的章会拆开翻
- 目录：侧栏树，点击跳到该章；当前章高亮
- 全屏：顶栏「全屏」或 F11；Esc 退出。全屏时顶栏收起，鼠标移到顶部再出现
- 记住进度：章节 href（可含 `#锚点`）+ 章内滚动比例（0～1），不存像素位置
- **绿色软件：** 打开的书会复制进程序目录下的 `data/library/`，进度、字体和 WebView 数据也在 `data/`。把整个文件夹拷走即带走书和状态。
- 字体：默认「使用原书字体」。关掉且衬线 / 无衬线 / 等宽 / 中文·CJK 四个文件都上传后，才强制用自定义字体（CJK 码位走中文/CJK 槽）。缺任何一个则仍按原书 CSS。**每槽一个文件**（Regular 或 Book 均可；不必上传 Italic / Bold / SemiBold）。覆盖开启后，书中的斜体和粗体由引擎合成，没有单独的斜体/粗体槽。请在字体面板上传；只把文件丢进 `data/fonts/` 不会生效（槽位要登记在 `settings.json`）。字体面板分两栏：原书 CSS 怎么写（含 `@font-face`；`src` 不在书内会标明），以及本章实际绘制。命名字体没装上只列 CSS 名；走 `serif` 等泛型时显示「（系统 serif）」，不把汉字猜成雅黑或宋体。

样书：`fixtures/sample.epub`。

## 接着要做

本地书架、书签、章内搜索、字号与浅色/暗色。进度字段里预留了 CFI，等分页引擎再填。

## 技术栈

| 层 | 技术 |
| --- | --- |
| 桌面壳 | Tauri 2（WebView2） |
| 界面 | React 19 + TypeScript + Vite |
| 领域与解析 | Rust：`iced-reader-core`、`iced-reader-epub`（rbook） |

业务逻辑进 Rust 核心，不进 Windows API。前端不直接拆 `.epub`。

## 环境

- Windows 11（已带 WebView2）
- [Rust](https://rustup.rs/)（MSVC 工具链）
- Visual Studio 2022 Build Tools，勾选「使用 C++ 的桌面开发」
- Node.js LTS

若本机开着 Smart App Control（强制），会拦住 Cargo 的未签名 build script，也会拦住本地编出来的 `IcedReader.exe`。开发前请先关闭。

## 开发

```powershell
npm install
.\scripts\dev.ps1
```

或：

```powershell
npm run tauri -- dev
```

调试时自动打开一本书：

```powershell
$env:ICED_READER_OPEN = "$PWD\fixtures\sample.epub"
.\scripts\dev.ps1
```

便携数据（相对 exe 所在目录）：

```
iced-reader.exe
data/
  library/          导入的书
  fonts/            字体面板写入的 serif/sans/mono/cjk 文件
  settings.json     阅读设置（含「使用原书字体」和各槽登记）
  progress.json     阅读进度
  webview/          WebView2 用户数据
```

开发时 exe 在 `target/debug/`，因此 `data/` 会出现在那里（已被 git 忽略）。

## 发布

先载入 MSVC（与 `scripts/dev.ps1` 相同，或在「x64 Native Tools」终端里）：

```powershell
npm run tauri -- build
```

编完后必须再拷一份**独立文件**（不要改名硬链接），供分发和 Everything 检索：

```powershell
Copy-Item target\release\iced-reader.exe target\release\IcedReader-0.1.1-windows-x64.exe
```

文件名：`IcedReader-{version}-windows-x64.exe`，版本号与 `src-tauri/tauri.conf.json` 的 `version` 一致。

| 产物 | 路径 |
| --- | --- |
| 分发用绿色 exe | `target/release/IcedReader-{version}-windows-x64.exe` |
| Cargo 原始 exe | `target/release/iced-reader.exe`（可留给工具链，不要当发布文件） |
| NSIS 安装包 | `target/release/bundle/nsis/` |
| MSI | `target/release/bundle/msi/` |

GitHub Release 上传那份 `IcedReader-…-windows-x64.exe`。拷到任意可写目录再运行，同级生成 `data/`。不要装进 Program Files，否则可能写不进 `data/`。系统仍需 WebView2（Win11 自带）。

## 测试

```powershell
cargo test -p iced-reader-core
cargo test -p iced-reader-epub
npx tsc --noEmit
```

## 目录

```
crates/core           格式无关的书模型、进度
crates/formats-epub   EPUB 适配器（rbook 不外泄到前端）
src-tauri             Tauri 命令、自定义协议 icedreader://
ui                    书架/阅读壳
fixtures              样书
scripts/dev.ps1       Windows 开发启动
```

给编码助手的仓库约定见仓库根目录的 [`AGENTS.md`](AGENTS.md)，新开对话会自动读入并遵循。

## 许可

MIT
