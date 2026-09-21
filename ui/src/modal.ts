import { useEffect, useLayoutEffect, useRef } from "react";

/**
 * 真 modal：面板本体用原生 `<dialog>` + `showModal()`。
 *
 * 这一步把模态语义交给浏览器，而不是靠一层 fixed 遮罩装样子——对话框进 top
 * layer 绘制（盖住顶栏，不受 z-index 影响），背景整层 inert（鼠标点不到、Tab
 * 进不去、读屏读不到），焦点圈在面板内部，Esc 走 dialog 自己的 `cancel`，关闭
 * 后焦点还给打开它的那个元素。手写 focus trap + aria-hidden 补不齐这些。
 *
 * 关闭只有两条明路：Esc（`cancel`）与面板自己的「关闭」/「取消」按钮。
 * **点遮罩不关**：背景既然是 inert，点在它上面语义上就该是「什么也没发生」；
 * 何况编辑元数据里可能正有没保存的输入，手一滑点到旁边就丢不合适。
 *
 * 用法：
 * ```tsx
 * const dialogRef = useModalDialog(() => onClose());
 * <dialog ref={dialogRef} className="…">…
 * ```
 * `onDismiss` 只在 Esc 时调用，允不允许关由它自己判断（比如保存中不关）。打开
 * 瞬间的落焦交给浏览器的焦点代理（第一个可聚焦子节点）；内容异步出现、要落到
 * 别处时，面板自己在内容就绪后再 focus 一次。
 */
export function useModalDialog(onDismiss: () => void) {
  const ref = useRef<HTMLDialogElement | null>(null);
  // 面板每次渲染都会给新的闭包，这里始终读最新的一份（下面的 effect 只在挂载
  // 时跑一次，不能把回调写进依赖里）。
  const dismiss = useRef(onDismiss);
  useEffect(() => {
    dismiss.current = onDismiss;
  });

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    // Esc 先落到 dialog 的 cancel 上：拦下浏览器默认的关闭，改走面板自己的
    // 关闭流程（保存中/读取中的判断在 onDismiss 里）。
    const onCancel = (e: Event) => {
      e.preventDefault();
      dismiss.current();
    };
    el.addEventListener("cancel", onCancel);
    // showModal 必须在布局阶段调用：等 paint 之后再开，会先闪一帧「没有遮罩、
    // 也没进 top layer」的普通卡片。
    if (!el.open) el.showModal();
    return () => {
      el.removeEventListener("cancel", onCancel);
      if (el.open) el.close();
    };
    // StrictMode 下会「挂载→卸载→再挂载」跑两遍，`el.open` 判断保证幂等。
  }, []);

  return ref;
}
