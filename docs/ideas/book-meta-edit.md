# 想法/进展：书名规范 = 编辑元数据 + 伴生 md

状态（2026-09-04 起稿，后续提交完成）：v1 全部落地。核心已提交 `e498e8f`；UI 面板（`BookMetaPanel`）、AGENTS.md 约定同步、tsc/cargo 验证在后续提交完成。**字段级分隔符 `" - "` 已由用户确认（2026-09-04）**，落地为 `core::book_meta::FIELD_SEP`，留作不同性质字段拼接用。剩余：真实脏书名样例与清洗规则扩充（用户后补）、桌面窗口手工核对、将来字段扩展。

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
   - 字段级分割 `" - "`（空格连字符空格）作为将来拼**不同性质字段**（如 `作者 - 书名`）的备用约定 —— **用户 2026-09-04 已确认**，常量 `core::book_meta::FIELD_SEP` 已落地（当前四字段同性质，尚未使用）。

## 实现现状（核心已提交 `e498e8f`；UI/文档在本提交完成）

- `crates/core/src/book_meta.rs`：`BookMeta` 结构 + `<stem>.md` 读写（`<!-- icedreader-meta` HTML 注释块、key: value、零依赖宽容解析、坏块忽略重建）；`clean_title`（折叠空白/全角空格、去首尾）；`join_title`（`TITLE_JOIN_SEP = " _ "`，空字段跳过）；`FIELD_SEP = " - "`（字段级分隔符，已确认，留作不同性质字段拼接）；`resolved_title`（裁决链）；带单测。
- `crates/core/src/lib.rs`：导出 `book_meta`（含 `FIELD_SEP`）。
- `src-tauri/src/book_meta.rs`：`BookMetaFields` / `BookMetaView` + `view_for`。`BookMetaView` 把 `confirmedTitle`（md 原始 displayTitle，空 = 未确认，供手改框绑定）与 `displayTitle`（当前裁决结果）分开；带单测。
- `src-tauri/src/lib.rs`：Tauri 命令 `get_book_meta` / `set_book_meta`（camelCase，已注册）。
- `src-tauri/src/library.rs`：`list_library` 与 `open_book` 统一走裁决链，书架与阅读标题一致；`delete_book` 连带删伴生 md（有测试：只删同名 md、无关 md 保留）。
- `ui/src/BookMetaPanel.tsx`（新）：书架三点菜单「编辑元数据…」→ 居中模态：`originalTitle` 只读、`title`/`subtitle`/`volume` 输入、拼接预览、显示名手改框（留空 = 派生）与「自动填充」（手改框非空时禁用，不覆盖手改）、「保存后书架将显示」实时行；保存 → `set_book_meta` → 重新 `list_library` 刷新书架。
- `ui/src/Library.tsx`：三点菜单加「编辑元数据…」（位于「从书库删除」上方）+ `onEditMeta` prop。
- `ui/src/types.ts` / `App.tsx` / `styles.css`：`BookMetaView`/`BookMetaFields` 类型、面板状态与接线、模态样式。
- `AGENTS.md`：新增「书元数据（书名规范）」小节（裁决链、伴生 md、符号禁则、删书联动）+ 命令表 + 书架菜单描述 + 验证段。

**验证状态**：`cargo test`（core/lib/epub）与 `npx tsc --noEmit` 全部通过（本提交）。桌面窗口手工核对尚未做（无窗口环境）。

## 已完成（下一步）

1. ✅ **UI 面板**：`Library.tsx` 三点菜单加「编辑元数据…」，模态面板含 title/subtitle/volume 输入、拼接预览与「自动填充」按钮、displayTitle 手改框（自动填充不覆盖手改）、originalTitle 只读展示；保存 → `set_book_meta` → 重新 `list_library` 刷新书架。
   - 交互关键设计：手改框初值 = md 里用户确认过的 displayTitle（未确认过则为空）；空 = 派生模式。`BookMetaView.confirmedTitle`（md 原始值）与 `displayTitle`（裁决结果）分开，避免首次保存把旧名锁死。自动填充仅在手改框为空时可点，把字段拼接写入框内；手改非空即锁定（编辑字段也不会覆盖）。
2. ✅ **AGENTS.md 同步**：新增「书元数据（书名规范）」小节（裁决链、伴生 md 约定、全角禁则与 `" _ "`/`" - "` 符号分工、删书联动）；Tauri 命令表补 `get_book_meta`/`set_book_meta`；书架菜单描述与验证段更新。
3. ✅ **验证**：`cargo test`（core 37 / lib 22 / epub 18+2 ignored 全过）、`npx tsc --noEmit` 无错。手工核对（书架/顶栏标题一致、删书连带删 md、面板交互手感）需桌面窗口。

## AGENTS.md 同步（已完成，原草案存档）

已按以下内容写入 AGENTS.md「书元数据（书名规范）」小节，原草案存档如下：

- 书架条目增加用户可编辑元数据，存 `data/library/<stem>.md`（与 epub 同名，程序维护，用户走 UI 不手编）。
- 显示名裁决链：md `displayTitle`（用户确认过）→ 字段拼接 → `dc:title`（非空且非 "Untitled"）→ 文件名；`list_library`/`open_book` 统一应用，书架与阅读标题一致。
- 程序生成符号一律 ASCII 禁全角；同性质拼接用 ` _ `、不同性质字段用 ` - `（已确认）；不转写原书自带字符。
- `delete_book` 连带删除伴生 md。
- 划线仍存 `annotations.json`，不进 md（v1）。

## 待补 / 待确认

- 真实脏书名样例（用户后补）——用于扩充清洗规则；当前 `clean_title` 只做保守空白折叠，不猜书名主体。
- 桌面窗口手工核对：面板字段布局与自动填充交互手感、Esc/遮罩关闭、书架与顶栏标题一致、删书连带删 md（AGENTS「验证」已列清单）。
- 后续可选：md 承载划线并支持导出；孤儿 md 扫描配对；作者/ISBN 等字段扩展（不同性质字段拼接用 `" - "`）。
