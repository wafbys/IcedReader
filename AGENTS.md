# Agent 说明

给在本仓库改代码的人（含编码助手）用。产品说明见 `README.md`。

## 这是什么

IcedReader 是桌面电子书阅读器。名字不是 Iced GUI。当前实现：Tauri 2 壳 + React 界面 + Rust 核心。Windows 先打磨；以后 Mac/Linux/iOS/鸿蒙，所以不要把业务写进 Windows 专用 API。

## 分层（不要打穿）

| 路径 | 可以做 | 不要做 |
| --- | --- | --- |
| `crates/core` | `Book` / `Locator` / 进度存储 | zip、OPF、Tauri、DOM |
| `crates/formats-epub` | 打开 EPUB、出 HTML 和资源 | UI、SQLite、直接给前端 rbook 类型 |
| `src-tauri` | 命令、文件对话框、`icedreader` 协议、把核心能力暴露出去 | 自己 parse EPUB、排版 |
| `ui` | 阅读壳、交互 | 读磁盘上的 epub 字节 |

新格式（TXT / PDF 等）只加适配器，实现 `Book` + `BookOpener`。前端命令形状尽量不动。

## 硬约束

1. **正文是 HTML。** 章节资源 URL 在 Rust 里改写成 `http://icedreader.localhost/book/{id}/...`（非 Windows 为 `icedreader://localhost/...`）。前端用 `srcDoc` 显示章节，图片/CSS 走自定义协议。
2. **进度只存 `Locator`：`href` + `fraction`（0～1）+ 可选 `cfi`。** 禁止存像素 `scrollTop`。CFI 等分页再填，先留字段。
3. **进度键：** 有 EPUB identifier 用 `id:...`；否则相对便携书库用 `lib:...`。禁止用会随目录搬家失效的绝对 `path:` 当主键（仅作没有书库目录时的回退）。实现见 `progress_key`。
4. **章节 iframe 不要开 `allow-scripts`。** 现在是 `allow-same-origin`，父页读滚动比例。不要为了省事给 EPUB 开脚本。
5. **IPC 用 camelCase**（Rust 结构体 `#[serde(rename_all = "camelCase")]`）。
6. **绿色软件：** 设置、进度、导入的书、WebView 缓存一律在 `{exe 目录}/data/`，禁止写入 `%APPDATA%` / 注册表当主存储。打开外部 EPUB 时复制进 `data/library/`（已在书库内则不再复制）。整个程序目录搬走即带走全部状态。目录必须可写，不要往 Program Files 里装。

## 常用命令

```powershell
npm install
.\scripts\dev.ps1
cargo test -p iced-reader-core
cargo test -p iced-reader-epub
cargo check -p iced-reader
npx tsc --noEmit
```

自动打开样书：`$env:ICED_READER_OPEN = "$PWD\fixtures\sample.epub"`。

Windows 编译需要 MSVC。`scripts/dev.ps1` 会载入 vsvars。不要用 `--offline` 除非依赖已经在本地。

## Tauri 命令

- `open_book` / `close_book` / `pending_book`
- `get_chapter` / `resource_origin`
- `save_progress`

协议：`src-tauri/src/protocol.rs`，scheme `icedreader`。Windows 上实际请求是 `http://icedreader.localhost/...`。

## 改 UI 时

- 阅读区必须铺满顶栏以下的客户区（flex：顶栏 `auto`，`.stage` `flex:1`，iframe `position:absolute; inset:0`）。不要用「两行 auto + 1fr」那种会把正文挤进中间空行的 grid。
- 界面文案默认中文。
- 提交信息用中文，说明做了什么、为什么。

## 不要做

- 不要把解析逻辑搬进 JS（包括让 foliate-js 直接 unzip）。分页若接 foliate-js，也只让它排版，书仍由 Rust 打开。
- 不要为了第一口去接 PDF/MOBI；接口预留即可。
- 不要提交 `target/`、`node_modules/`、`ui/dist/`、`src-tauri/gen/`、`data/`、`fixtures/verify-*.png`。
- 不要在文档或代码里写用户的密钥、本机绝对路径（样例用仓库相对路径）。

## 验证

改阅读功能时：打开 `fixtures/sample.epub`，确认第一章中文、下一章、关开后进度还在。改布局时确认顶栏以下没有空白条。没有桌面窗口时至少跑 `cargo test -p iced-reader-epub`。
