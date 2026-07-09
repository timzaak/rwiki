import { useEffect, useRef } from "react";

const apiUrl = import.meta.env.VITE_RWIKI_API_URL as string;

// 外部 widget 脚本（frontend 构建的 rwiki-chat.js）暴露的全局 init 选项。
// 字段与 configuration 文档的 Widget 配置一致。
type RWikiChatOptions = {
  apiUrl: string;
  channelId: string | string[];
  locale?: string;
  title?: string;
  primaryColor?: string;
  position?: "left" | "right";
  welcomeMessage?: string;
  messages?: Record<string, string>;
  suggestedQuestions?: string[] | Record<string, string[]>;
};

type RWikiChatGlobal = {
  init?: (opts: RWikiChatOptions) => void;
  destroy?: () => void;
  setLocale?: (locale: string) => void;
};

type WindowWithRWikiChat = Window & { RWikiChat?: RWikiChatGlobal };

/**
 * 挂载 RWiki Chat Widget。
 *
 * 语言由 docs-web 当前 locale 决定（来自 URL 路径段）：首次 init 时传入，
 * 之后 locale 变化时通过 setLocale 实时切换，不重建 widget、不丢失对话。
 */
export default function RWikiChatWidget({ locale }: { locale: string }) {
  const firstRun = useRef(true);
  // 始终保存最新 locale：脚本异步加载时，onload 里的 init 读取此 ref，
  // 避免用到挂载时闭包捕获的过期 locale（修复 locale 在脚本加载期间变化的竞态）。
  const localeRef = useRef(locale);
  localeRef.current = locale;

  // 加载脚本并初始化一次。
  useEffect(() => {
    const w = window as unknown as WindowWithRWikiChat;

    const initWidget = () => {
      w.RWikiChat?.init?.({
        apiUrl,
        channelId: "help_center",
        primaryColor: "#7c3aed",
        locale: localeRef.current,
      });
    };

    if (w.RWikiChat?.init) {
      initWidget();
    } else {
      const script = document.createElement("script");
      script.src = `${apiUrl}/widget/rwiki-chat.js`;
      script.onload = initWidget;
      document.body.appendChild(script);
    }

    return () => {
      w.RWikiChat?.destroy?.();
    };
    // 仅在挂载时初始化；locale 变化由下方 effect 处理。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // locale 变化时实时切换 widget 语言。
  useEffect(() => {
    // 首次运行跳过：初始 locale 已在 init 中传入，且此时脚本可能尚未加载。
    if (firstRun.current) {
      firstRun.current = false;
      return;
    }
    const w = window as unknown as WindowWithRWikiChat;
    w.RWikiChat?.setLocale?.(locale);
  }, [locale]);

  return null;
}
