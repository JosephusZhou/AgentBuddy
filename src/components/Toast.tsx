import { useEffect, useState } from "react";
import { createPortal } from "react-dom";

/** 进入/退出动画时长，需与 CSS `.ui-toast` 的 transition 时长保持一致。 */
const ANIM_MS = 240;

/**
 * 顶部滑入式浮层提示。
 *
 * 通过 portal 挂到 `document.body`，脱离页面文档流（`position: fixed`），
 * 因此显示/消失不会引起页面内容上下抖动。配合 `useStatusMessage`：
 * `message` 非空即滑入显示，置空即滑出并在动画结束后卸载。
 */
export function Toast({ message }: { message: string }) {
  // `text` 是当前正在展示（含退出动画阶段）的文案；`visible` 驱动 CSS 过渡。
  const [text, setText] = useState("");
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    if (message) {
      // 先挂载在隐藏态，再于下一帧切到可见态，确保进入过渡能够触发。
      setText(message);
      const raf = requestAnimationFrame(() => setVisible(true));
      return () => cancelAnimationFrame(raf);
    }
    // 置空：先播放退出动画，动画结束后再卸载节点。用定时器而非 transitionend，
    // 避免 prefers-reduced-motion 等场景下过渡不触发导致提示卡住。
    setVisible(false);
    const timer = setTimeout(() => setText(""), ANIM_MS);
    return () => clearTimeout(timer);
  }, [message]);

  if (!text) return null;

  return createPortal(
    <div className="ui-toast-layer" aria-live="polite">
      <div className={`ui-toast${visible ? " is-visible" : ""}`} role="status">
        {text}
      </div>
    </div>,
    document.body,
  );
}
