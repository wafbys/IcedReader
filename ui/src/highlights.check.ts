// 划线摘录的自检（仓库没有前端测试 runner，用 node 直接跑这个模块）。
// Run: node ui/src/highlights.check.ts   （Node 22.6+ 直接剥离类型）
//
// 钉住的不变量：摘录必须**完整**保留整段选区。任何长度上限都会把长摘抄的中间
// 从伴生 md 里悄悄丢掉 —— 旧实现就是 160 字取「头 80 + … + 尾 80」，长摘抄的
// 中段整段消失，而 md 里那句摘抄行看起来仍像一段完整引用。
import { excerptFromSelection } from "./highlights.ts";

let failures = 0;
const ok = (label: string, cond: boolean) => {
  if (cond) console.log(`ok   ${label}`);
  else {
    failures += 1;
    console.log(`FAIL ${label}`);
  }
};

// 短选区原样保留。
ok("短选区原样", excerptFromSelection("你好，世界。") === "你好，世界。");
// 跨段选区：换行与缩进折成一个空格，不留换行（md 的 text: 字段必须单行）。
ok(
  "跨段折叠为单行",
  excerptFromSelection(" 第一段。\n\n   第二段。 ") === "第一段。 第二段。",
);
// 首尾空白裁掉（拖选常带行尾空格）。
ok("首尾空白裁掉", excerptFromSelection("\n  文字  \n") === "文字");
// 长选区一字不少。
ok("500 字不截断", excerptFromSelection("字".repeat(500)).length === 500);
// 中段必须真的在里面（回归位：曾按 160 取头尾，中段整段丢失）。
const mid = `头${"甲".repeat(200)}中段在这里${"乙".repeat(200)}尾`;
const got = excerptFromSelection(mid);
ok("中段不丢", got.includes("中段在这里") && got.length === mid.length);
// 省略号只可能来自选区自己，程序不再合成（旧实现会拼一个 …）。
ok("不合成省略号", !excerptFromSelection("甲".repeat(400)).includes("…"));
// 空白选区 → 空串（调用方据此放弃这次划线，不会写进 md）。
ok("空选区为空", excerptFromSelection("   \n ") === "");

if (failures > 0) throw new Error(`${failures} highlight check failures`);
console.log("ALL OK");
