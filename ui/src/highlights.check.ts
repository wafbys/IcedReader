// 划线摘录的自检（仓库没有前端测试 runner，用 node 直接跑这个模块）。
// Run: node ui/src/highlights.check.ts   （Node 22.6+ 直接剥离类型）
//
// 钉住两条不变量：
//   1. 摘录是**整段选区**，不做长度截断 —— 旧实现是 160 字取「头 80 + … + 尾
//      80」，长摘抄的中间整段从伴生 md 里消失，而摘抄行看起来仍像完整引用。
//   2. 段落换行**原样保留**（Rust 侧据此切段：机器字段折成一行存 `text:`，段落
//      切点存 `paras:`，人读的摘抄行一条一段 `> ` 引用）。
import { excerptFromSelection } from "./highlights.ts";

let failures = 0;
const eq = (label: string, actual: unknown, expected: unknown) => {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a !== e) {
    failures += 1;
    console.log(`FAIL ${label}: got ${a}, want ${e}`);
  } else {
    console.log(`ok   ${label} = ${a}`);
  }
};

// 短选区原样保留。
eq("短选区原样", excerptFromSelection("你好，世界。"), "你好，世界。");
// 段内空白折成一个空格（拖选常带行尾空格与源里的排版空白）。
eq("段内空白折成一个空格", excerptFromSelection("  第一段   有   空格  "), "第一段 有 空格");
// 段落换行保留：三段还是三段。
eq(
  "段落换行保留",
  excerptFromSelection("第一段。\n  第二段。\n\n第三段。"),
  "第一段。\n第二段。\n第三段。",
);
// 空段丢弃（选区里夹着空行不该产生空引用行）。
eq("空段丢弃", excerptFromSelection("甲\n\n  \n乙"), "甲\n乙");
// 长选区一字不少（回归位：曾按 160 取头尾）。
eq("500 字不截断", excerptFromSelection("字".repeat(500)).length, 500);
// 中段必须真的在里面（回归位：中段曾整段丢失）。
const mid = `头${"甲".repeat(200)}中段在这里${"乙".repeat(200)}尾`;
const got = excerptFromSelection(mid);
eq("中段不丢", got.includes("中段在这里") && got.length === mid.length, true);
// 省略号只可能来自选区自己，程序不再合成（旧实现会拼一个 …）。
eq("不合成省略号", excerptFromSelection("甲".repeat(400)).includes("…"), false);
// 空白选区 → 空串（调用方据此放弃这次划线，不写进 md）。
eq("空白选区为空", excerptFromSelection("  \n \t "), "");

if (failures > 0) throw new Error(`${failures} highlight check failures`);
console.log("ALL OK");
