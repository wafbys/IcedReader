# 想法/进展：书名规范 = 编辑元数据 + 伴生 md

状态（2026-09-04 起稿，多提交推进）：v1 核心+UI 已落地；**md v2（author/year/publisher/isbn 字段与拼接模板）本提交落地**。`" - "` 字段级分隔符（2026-09-04 确认）正式成为拼接模板的主体分隔。剩余：真实脏书名样例与清洗规则扩充（用户后补）、桌面窗口手工核对、作者行联动/自动预填等可选增强。

## 目标

把模糊的「书名规范」收敛为：给书架条目加一层**用户可编辑的元数据**，显示名裁决链为 `手填显示名 → 字段拼接 → dc:title → 文件名`；元数据存成与 epub 同名的伴生 Markdown（程序维护，用户只走 UI，不手编 md）。初期**不做**自动抓取（豆瓣/亚马逊等），用户后补真实脏样例再扩充清洗规则。

## 已拍板决策

1. **入口**：书架封面右下三点菜单加「编辑元数据…」，置于「从书库删除」上方；面板标题可用「书名规范 / 编辑书籍信息」。
2. **字段宁少勿多 → v2 扩展**：初版 `title` / `subtitle` / `volume` / `displayTitle`；2026-09-04 用户拍板扩展为 `title` / `subtitle` / `volume` / `author` / `year` / `publisher` / `isbn` / `displayTitle`（作者、ISBN、出版年份、出版社加入）。
3. **md 由程序维护**：不是用户手写格式；用户编辑一律走 UI 面板。md 人类可读只是可拷贝/备份/进 git 的福利。
4. **md v1 不含划线**：划线仍存 `data/annotations.json`；将来「md 存划线 / 导出笔记」另起版本。
5. **裁决链（写进 AGENTS）**：用户确认过的 `displayTitle` → 字段自动拼接 → `dc:title`（非空且非 "Untitled"）→ 文件名兜底。手改的显示名永不被子段自动拼接覆盖。书架、顶栏、排序、删除确认共用同一结果，不分裂。
6. **originalTitle**：首次导入时程序见到的书名。存量书（功能上线前已入库）取功能上线后**第一次保存时**程序见到的书名。
7. **删除书连带删 `<stem>.md`**。
8. **符号约定（全角禁则，v2 调整）**：
   - 程序生成的符号一律 ASCII，**绝不产出全角**；原书 `dc:title` 自带的字符（含 `Ⅲ`、全角冒号等）原样保留，不转半角（那是书名的一部分）。
   - **拼接模板**（用户 2026-09-04 拍板）：`书名 [ _ 副标题] [ - 卷册] [ - 作者] [ - 出版年份] [ - 出版社] [ - ISBN]`。下划线 `" _ "` **只出现在书名与副标题之间**（初版曾用于 title/subtitle/volume 同性质拼接，v2 改为仅书名-副标题一处）；卷册起一律用字段级分隔 `" - "`（空格连字符空格）。空字段整体跳过，绝不出现两分隔符夹空段；书名必填，留空不拼接。
   - ISBN 填号码，拼入时自动补 ASCII `ISBN ` 前缀（值已以 ISBN 开头则保留原样）。

## 实现现状（v1 已提交 `e498e8f` + UI 提交；md v2 与拼接模板在本提交）

- `crates/core/src/book_meta.rs`：`BookMeta` 结构 + `<stem>.md` 读写（v2 字段 author/year/publisher/isbn；`<!-- icedreader-meta` HTML 注释块、key: value、零依赖宽容解析、坏块忽略重建，v1 md 读进 v2 字段为空不丢数据）；`clean_title`（折叠空白/全角空格、去首尾）；`join_title`（v2 拼接模板：`TITLE_JOIN_SEP = " _ "` 仅书名↔副标题、`FIELD_SEP = " - "` 卷册起、ISBN 自动补 `ISBN_LABEL` 前缀、书名必填）；`resolved_title`（裁决链）；带单测。
- `crates/core/src/lib.rs`：导出 `book_meta`（含 `FIELD_SEP`）。
- `src-tauri/src/book_meta.rs`：`BookMetaFields` / `BookMetaView`（v2 字段）+ `view_for`（作者预填原书 dc:creator，多名用、连接）。`BookMetaView` 把 `confirmedTitle`（md 原始 displayTitle，空 = 未确认，供手改框绑定）与 `displayTitle`（当前裁决结果）分开；带单测。
- `src-tauri/src/lib.rs`：Tauri 命令 `get_book_meta` / `set_book_meta`（camelCase，已注册；set 清洗全部 v2 字段）。
- `src-tauri/src/library.rs`：`list_library` 与 `open_book` 统一走裁决链，书架与阅读标题一致；`delete_book` 连带删伴生 md（有测试）。
- `ui/src/BookMetaPanel.tsx`：v2 八输入布局（主书名必填校验、作者预填提示、ISBN 自动前缀说明），拼接预览镜像 join_title，操作行常驻模态底部（长表单独立滚动）；显示名手改框（留空 = 派生）与「自动填充」（手改非空禁用）不变。
- `AGENTS.md`：书元数据小节更新为 v2 字段与拼接模板；验证段同步。

**验证状态**：本提交 `cargo test`（core 37 / lib 23 / epub 18+2 ignored）与 `npx tsc --noEmit`、`vite build` 全部通过。桌面窗口手工核对尚未做（无窗口环境）。

## 已完成（下一步）

1. ✅ **UI 面板**：`Library.tsx` 三点菜单加「编辑元数据…」，模态面板含 title/subtitle/volume 输入、拼接预览与「自动填充」按钮、displayTitle 手改框（自动填充不覆盖手改）、originalTitle 只读展示；保存 → `set_book_meta` → 重新 `list_library` 刷新书架。
   - 交互关键设计：手改框初值 = md 里用户确认过的 displayTitle（未确认过则为空）；空 = 派生模式。`BookMetaView.confirmedTitle`（md 原始值）与 `displayTitle`（裁决结果）分开，避免首次保存把旧名锁死。自动填充仅在手改框为空时可点，把字段拼接写入框内；手改非空即锁定（编辑字段也不会覆盖）。
2. ✅ **AGENTS.md 同步**：新增「书元数据（书名规范）」小节（裁决链、伴生 md 约定、全角禁则与 `" _ "`/`" - "` 符号分工、删书联动）；Tauri 命令表补 `get_book_meta`/`set_book_meta`；书架菜单描述与验证段更新。
3. ✅ **验证**：`cargo test`（core 37 / lib 22 / epub 18+2 ignored 全过）、`npx tsc --noEmit` 无错。手工核对（书架/顶栏标题一致、删书连带删 md、面板交互手感）需桌面窗口。
4. ✅ **md v2：字段扩展 + 新拼接模板**（用户 2026-09-04 拍板）：加 `author` / `year` / `publisher` / `isbn`；拼接模板 `书名 _ 副标题 - 卷册 - 作者 - 出版年份 - 出版社 - ISBN`（下划线仅书名↔副标题一处，卷册起全用 ` - `）；空段整体跳过不产生连续分隔符；书名必填（UI 禁保存）；ISBN 自动补 ASCII `ISBN ` 前缀；作者预填原书 dc:creator。core/src-tauri/UI/AGENTS/文档全部同步，单测与 tsc 通过。

## AGENTS.md 同步（已完成，原草案存档）

已按以下内容写入 AGENTS.md「书元数据（书名规范）」小节，原草案存档如下（v1 草案；v2 的拼接模板/分隔规则以「已拍板决策 8」与 AGENTS 现文为准）：

- 书架条目增加用户可编辑元数据，存 `data/library/<stem>.md`（与 epub 同名，程序维护，用户走 UI 不手编）。
- 显示名裁决链：md `displayTitle`（用户确认过）→ 字段拼接 → `dc:title`（非空且非 "Untitled"）→ 文件名；`list_library`/`open_book` 统一应用，书架与阅读标题一致。
- 程序生成符号一律 ASCII 禁全角；同性质拼接用 ` _ `、不同性质字段用 ` - `（已确认）；不转写原书自带字符。
- `delete_book` 连带删除伴生 md。
- 划线仍存 `annotations.json`，不进 md（v1）。

## 待补 / 待确认

- 真实脏书名样例（用户后补）——用于扩充清洗规则；当前 `clean_title` 只做保守空白折叠，不猜书名主体。
- 桌面窗口手工核对：长表单滚动与底部常驻按钮、必填校验、拼接预览实时性、作者预填、Esc/遮罩关闭、书架与顶栏标题一致、删书连带删 md（AGENTS「验证」已列清单）。
- **未拍板**：书架/顶栏第二行作者目前仍显示原书 dc:creator，而标题已按模板含 md 作者 —— 是否改为 md 作者优先（或作者行消隐）待定。
- 后续可选：出版社/ISBN/年份从原书 dc:publisher、identifier 自动预填；md 承载划线并支持导出；孤儿 md 扫描配对。
