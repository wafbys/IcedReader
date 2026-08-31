# IcedReader

Windows 优先的桌面电子书阅读器。产品名沿用 IcedReader，技术栈是 **Tauri 2 + React + Rust**，正文用系统 WebView 渲染 EPUB 的 HTML。

当前目标平台顺序：Windows → macOS / Linux → iOS → 鸿蒙。第一版只出品 EPUB，格式层按可扩展接口来写。

## 现在能做什么

- 打开本地 `.epub`（EPUB 2 / 3）
- 显示书名、作者
- 整章滚动阅读，上一章 / 下一章（方向键也可）
- 记住进度：章节 href + 章内滚动比例（0～1），不存像素位置
- **绿色软件：** 打开的书会复制进程序目录下的 `data/library/`，进度和 WebView 数据也在 `data/`。把整个文件夹拷走即带走书和状态。

样书：`fixtures/sample.epub`。

## 接着要做

本地书架、目录跳转、书签、章内搜索、字号与浅色/暗色。进度字段里预留了 CFI，等分页引擎再填。

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

便携数据（相对 `IcedReader.exe` 所在目录）：

```
IcedReader.exe
data/
  library/          导入的书
  progress.json     阅读进度
  webview/          WebView2 用户数据
```

开发时 exe 在 `target/debug/`，因此 `data/` 会出现在那里（已被 git 忽略）。

发布请分发「exe + 同级可写目录」，不要装进 Program Files，否则可能写不进 `data/`。系统仍需 WebView2（Win11 自带）。

## 测试

```powershell
cargo test -p iced-reader-core
cargo test -p iced-reader-epub
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
