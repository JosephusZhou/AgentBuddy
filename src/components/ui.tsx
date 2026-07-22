/** 跨页面共享的 UI 基元。 */

import { useRef, type MouseEvent } from "react";

/** 自定义复选框 `.ui-check-box` 内的对勾图形（ClaudeEnv / SkillsManage 共用，避免各自内联重复）。 */
export const CheckGlyph = () => (
  <span className="ui-check-box" aria-hidden="true">
    <svg
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <polyline points="3.5 8.5 6.5 11.5 12.5 5" />
    </svg>
  </span>
);

/**
 * 遮罩层「点击空白处关闭」的正确实现，供所有 `modal-overlay` 复用。
 *
 * 背景：若直接在遮罩层上用 `onClick` 判断 `e.target === e.currentTarget` 来关闭，会有一个缺陷——
 * 当用户在弹窗内的输入框按下鼠标、拖动到遮罩层再松开时（典型场景：全选输入框文字时把鼠标甩出框外松手），
 * `click` 事件会在「按下点与松开点最近的公共祖先」上触发，而这个祖先恰好是遮罩层本身，
 * 于是被误判为「点击遮罩层」并意外关闭弹窗。
 *
 * 修复：分别监听 `mousedown` / `mouseup`，仅当二者都发生在遮罩层自身、且为鼠标主键时才关闭。
 * 这样任意一端落在弹窗内部的拖拽都不会误关；同时仅响应主键，保持与原 `onClick`（左键）一致的语义。
 *
 * 用法：
 *   const dismiss = useOverlayDismiss(() => setOpen(false), !busy);
 *   <div className="modal-overlay ..." {...dismiss}> ... </div>
 *
 * @param onDismiss 关闭弹窗的回调
 * @param enabled   为 `false` 时禁用关闭（例如保存 / 删除进行中），默认 `true`
 */
export function useOverlayDismiss(onDismiss: () => void, enabled = true) {
  // 记录本次按下是否落在遮罩层自身；跨 mousedown → mouseup 保持，故用 ref 而非 state。
  const pressedSelf = useRef(false);
  return {
    onMouseDown: (e: MouseEvent<HTMLDivElement>) => {
      // 仅当「鼠标主键按在遮罩层本身」时记录；从弹窗内部冒泡上来的按下（target 为子元素）不算。
      pressedSelf.current = e.button === 0 && e.target === e.currentTarget;
    },
    onMouseUp: (e: MouseEvent<HTMLDivElement>) => {
      const shouldDismiss =
        e.button === 0 && pressedSelf.current && e.target === e.currentTarget;
      // 无论是否关闭都复位，避免状态残留影响下一次交互。
      pressedSelf.current = false;
      if (enabled && shouldDismiss) onDismiss();
    },
  };
}
