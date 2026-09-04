# 想法/进展：书名规范 = 编辑元数据 + 伴生 md

状态（2026-09-04）：设计决策已定稿；核心实现已提交 `e498e8f`（core 伴生 md 与裁决链、src-tauri 命令与删书联动）；**未完成**：书架三点菜单「编辑元数据…」UI 面板、AGENTS.md 约定同步、tsc 验证。本文档是跨机继续的交接依据，后续若调整在此更新。

## 目标

把模糊的「书名规范」收敛为：给书架条目加一层**用户可编辑的元数据**，显示名裁决链为 `手填显示名 → 字段拼接 → dc:title → 文件名`；元数据存成与 epub 同名的伴生 Markdown（程序维护，用户只走 UI，不手编 md）。初期**不做**自动抓取（豆瓣/亚马逊等），用户后补真实脏样例再扩充清洗规则。

## 已拍板决策

1. **入口**：书架封面右下三点菜单加「编辑元数据…」，置于「从书库删除」上方；面板标题可用「书名规范 / 编辑书籍信息」。
2. **字段宁少勿多**：先 `title` / `subtitle` / `volume` / `displayTitle` 四个；作者、ISBN 等后续扩展（md 与命令结构预留扩展位，先不做）。
3. **md 由程序维护**：不是用户手写格式；用户编辑一律走 UI 面板。md 人类可读只是可拷贝/备份/进 git 的福利。
4. **md v1 不含划线**：划线仍存 `data/annotations.json`；将来「md 存划线 / 导出笔记」另起版本。
5. **裁决链（写进 AGENTS）**：用户确认过的 `displayTitle` → 字段自动拼接 → `dc:title`（非空且非 "Untitled"）→ 文件名兜底。手改的显示名永不被子段自动拼接覆盖。书架、顶栏、排序、删除确认共用同一结果，不分裂。
6. **originalTitle**：首次导入时程序见到的书名。存量书（功能上线前已入库）取功能上线后**第一次保存时**程序见到的书名。
7. **删除书连带删 `<stem>.md`**。
8. **符号约定（全角禁则）**：
   - 程序生成的符号一律 ASCII，**绝不产出全角**；原书 `dc:title` 自带的字符（含 `Ⅲ`、全角冒号等）原样保留，不转半角（那是书名的一部分）。
   - 主书名/副标题/卷册拼接用 `" _ "`（空格下划线空格，用户 2026-09-04 拍板「用空格下划线空格，和字段分割区分开」），例：`三体 _ 死神永生`。
   - 字段级分割 `" - "`（空格连字符空格）作为将来拼**不同性质字段**（如 `作者 - 书名`）的备用约定 —— **待用户最终确认**（对话中断在该确认之前）。

## 实现现状（已提交 `e498e8f`，工作区 clean）

- `crates/core/src/book_meta.rs`：`BookMeta` 结构 + `<stem>.md` 读写（`<!-- icedreader-meta` HTML 注释块、key: value、零依赖宽容解析、坏块忽略重建）；`clean_title`（折叠空白/全角空格、去首尾）；`join_title`（`TITLE_JOIN_SEP = " _ "`，空字段跳过）；`resolved_title`（裁决链）；带单测。
- `crates/core/src/lib.rs`：导出 `book_meta`。
- `src-tauri/src/book_meta.rs`：`BookMetaFields` / `BookMetaView` + `view_for`（从 profile 自动带出待编辑字段与拼接预览）；带单测。
- `src-tauri/src/lib.rs`：Tauri 命令 `get_book_meta` / `set_book_meta`（camelCase，已注册）。
- `src-tauri/src/library.rs`：`list_library` 与 `open_book` 统一走裁决链，书架与阅读标题一致；`delete_book` 连带删伴生 md（有测试：只删同名 md、无关 md 保留）。

**验证状态**：代码内带单测，但本机**未跑** `cargo test` / `tsc --noEmit` —— 继续前先按 AGENTS「常用命令」跑一遍。

## 未完成（下一步）

1. **UI 面板**：`Library.tsx` 三点菜单加「编辑元数据…」，模态面板含 title/subtitle/volume 输入、拼接预览与「自动填充」按钮、displayTitle 手改框（自动填充不覆盖手改）、originalTitle 只读展示；保存 → `set_book_meta` → 重新 `list_library` 刷新书架。
2. **AGENTS.md 同步**：把裁决链、伴生 md 约定、全角禁则、删书联动写入（草案见下，需用户过目）。
3. **验证**：`cargo test -p iced-reader-core` / `-p iced-reader-epub` / `-p iced-reader`、`npx tsc --noEmit`；手工核对书架/顶栏标题一致、删书连带删 md。

## AGENTS.md 同步草案（待用户过目）

- 书架条目增加用户可编辑元数据，存 `data/library/<stem>.md`（与 epub 同名，程序维护，用户走 UI 不手编）。
- 显示名裁决链：md `displayTitle`（用户确认过）→ 字段拼接 → `dc:title`（非空非 "Untitled"）→ 文件名；`list_library`/`open_book` 统一应用，书架与阅读标题一致。
- 程序生成符号一律 ASCII 禁全角；主副/卷册拼接用 ` _ `；不转写原书自带字符。
- `delete_book` 连带删除伴生 md。
- 划线仍存 `annotations.json`，不进 md（v1）。

## 待补 / 待确认

- 符号分工最终确认：join ` _ ` 已实现；字段级 ` - ` 备用约定未最终确认。
- 真实脏书名样例（用户后补）——用于扩充清洗规则；当前 `clean_title` 只做保守空白折叠，不猜书名主体。
- 面板 UI 交互细节（字段布局、自动填充交互）。
- 后续可选：md 承载划线并支持导出；孤儿 md 扫描配对；作者/ISBN 等字段扩展。
