import { useEffect } from "react";

const apiUrl = import.meta.env.VITE_RWIKI_API_URL as string;

// 外部 widget 脚本（frontend 构建的 rwiki-chat.js）暴露的全局 init 选项。
// 字段与 configuration 文档的 Widget 配置一致。
type RWikiChatOptions = {
  apiUrl: string;
  title?: string;
  primaryColor?: string;
  position?: "left" | "right";
  welcomeMessage?: string;
  suggestedQuestions?: string[] | Record<string, string[]>;
};

export default function RWikiChatWidget() {
  useEffect(() => {
    const w = window as unknown as {
      RWikiChat?: {
        init?: (opts: RWikiChatOptions) => void;
        destroy?: () => void;
      };
    };

    const initWidget = () => {
      w.RWikiChat?.init?.({ apiUrl, primaryColor: "#7c3aed" });
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
  }, []);

  return null;
}
