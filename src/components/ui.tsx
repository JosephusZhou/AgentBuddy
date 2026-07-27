/** 跨页面共享的 UI 基元。 */

import { useEffect, useRef, type MouseEvent } from "react";

const FOCUSABLE_SEL =
  'input:not([type="hidden"]):not(:disabled), textarea:not(:disabled), select:not(:disabled), button:not(:disabled), [tabindex]:not([tabindex="-1"])';

/**
 * 全局弹窗可访问性（在 App 根部调用一次，各页面零接入）：
 *  - 任一 .modal-overlay 变为 visible 时，自动聚焦其主输入框（无输入框则第一个可交互元素）；
 *  - 弹窗打开期间圈定 Tab / Shift+Tab 在其内部循环，避免焦点落到被遮罩的背景页面。
 *
 * 通过 MutationObserver 监听 .modal-overlay 的 class 变化，与页面组件的
 * 打开状态变量解耦；同一时刻只处理第一个可见弹窗（本应用不叠加弹窗）。
 */
export function useGlobalModalA11y() {
  useEffect(() => {
    let activeModal: HTMLElement | null = null;

    const isVisible = (n: HTMLElement) =>
      n.offsetParent !== null || n === document.activeElement;
    const focusablesOf = (el: HTMLElement) =>
      Array.from(el.querySelectorAll<HTMLElement>(FOCUSABLE_SEL)).filter(isVisible);

    const sync = () => {
      const modal =
        document.querySelector(".modal-overlay.visible .modal") as HTMLElement | null;
      if (modal === activeModal) return;
      activeModal = modal;
      if (!modal) return;
      // 初始焦点：优先第一个文本输入框，否则第一个可交互元素。
      const firstInput = Array.from(
        modal.querySelectorAll<HTMLElement>(
          'input:not([type="hidden"]):not(:disabled), textarea:not(:disabled)'
        )
      ).find(isVisible);
      (firstInput ?? focusablesOf(modal)[0])?.focus();
    };

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== "Tab" || !activeModal) return;
      const items = focusablesOf(activeModal);
      if (items.length === 0) return;
      const first = items[0];
      const last = items[items.length - 1];
      const active = document.activeElement as HTMLElement | null;
      const inside = active !== null && activeModal.contains(active);
      if (e.shiftKey && (!inside || active === first)) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && (!inside || active === last)) {
        e.preventDefault();
        first.focus();
      }
    };

    const observer = new MutationObserver(sync);
    observer.observe(document.body, {
      subtree: true,
      attributes: true,
      attributeFilter: ["class"],
    });
    // capture 阶段监听，保证在其它 keydown 处理（如 Esc 关闭）之前圈定焦点。
    document.addEventListener("keydown", onKeyDown, true);
    sync();
    return () => {
      observer.disconnect();
      document.removeEventListener("keydown", onKeyDown, true);
    };
  }, []);
}

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
