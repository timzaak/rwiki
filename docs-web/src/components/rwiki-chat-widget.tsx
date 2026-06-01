import { useEffect } from "react";

const apiUrl = import.meta.env.VITE_RWIKI_API_URL as string;

export default function RWikiChatWidget() {
  useEffect(() => {
    const w = window as unknown as {
      RWikiChat?: {
        init?: (opts: { apiUrl: string }) => void;
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
