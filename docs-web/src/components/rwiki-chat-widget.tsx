import { useEffect, useRef } from "react";

import {
  apiUrl,
  channelIdForLocale,
  getRWikiChat,
  RWIKI_CHAT_READY_EVENT,
} from "@/lib/rwiki";

/**
 * 挂载 RWiki Chat Widget。
 *
 * 语言由 docs-web 当前 locale 决定（来自 URL 路径段）：首次 init 时传入，
 * 之后 locale 变化时通过 setLocale 实时切换，不重建 widget、不丢失对话。
 * 频道跟随语言（映射表见 @/lib/rwiki，每种站点语言一个频道）：setChannel 同步
 * 切换，切换频道会开启新会话（widget 既定语义）。旧版 widget 没有
 * setChannel 时静默跳过（停留在初始频道）。
 */
export default function RWikiChatWidget({ locale }: { locale: string }) {
  const firstRun = useRef(true);
  // 始终保存最新 locale：脚本异步加载时，onload 里的 init 读取此 ref，
  // 避免用到挂载时闭包捕获的过期 locale（修复 locale 在脚本加载期间变化的竞态）。
  const localeRef = useRef(locale);
  localeRef.current = locale;

  // 加载脚本并初始化一次。
  useEffect(() => {
    const initWidget = () => {
      getRWikiChat()?.init?.({
        apiUrl,
        channelId: channelIdForLocale(localeRef.current),
        primaryColor: "#7c3aed",
        locale: localeRef.current,
        // 文档站用右侧停靠侧栏（推挤正文，不遮挡），替代默认居中弹窗。
        displayMode: "sidePanel",
      });
      // 通知搜索等消费者全局 API 已就绪：它们渲染期的快照不会自行刷新。
      window.dispatchEvent(new CustomEvent(RWIKI_CHAT_READY_EVENT));
    };

    if (getRWikiChat()?.init) {
      initWidget();
    } else {
      const script = document.createElement("script");
      script.src = `${apiUrl}/widget/rwiki-chat.js`;
      script.onload = initWidget;
      document.body.appendChild(script);
    }

    return () => {
      getRWikiChat()?.destroy?.();
    };
    // 仅在挂载时初始化；locale 变化由下方 effect 处理。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // locale 变化时实时切换 widget 语言与频道。
  useEffect(() => {
    // 首次运行跳过：初始 locale 已在 init 中传入，且此时脚本可能尚未加载。
    if (firstRun.current) {
      firstRun.current = false;
      return;
    }
    const rwikiChat = getRWikiChat();
    rwikiChat?.setLocale?.(locale);
    rwikiChat?.setChannel?.(channelIdForLocale(locale));
  }, [locale]);

  return null;
}
