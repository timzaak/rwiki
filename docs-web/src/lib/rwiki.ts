import { i18n } from "./i18n";

// rwiki 后端的共享配置：搜索与聊天 widget 都从这里取 apiUrl / 频道映射，
// 保证「搜索的频道」和「聊天的频道」始终一致。
export const apiUrl = import.meta.env.VITE_RWIKI_API_URL as string;

// 每种站点语言一个知识库频道：加语言 = 加一行，且该频道须在后端
// [channels.<id>] 存在；辅语言频道在后端配自己的 content_language。
// 灌库见 scripts/demo-start.py / seed-production.py。
const CHANNEL_BY_LOCALE: Record<string, string> = {
  en: "help_center",
  zh: "help_center_zh",
};

// 未列出的 locale 回退到默认语言的频道。
export function channelIdForLocale(locale: string): string {
  return CHANNEL_BY_LOCALE[locale] ?? CHANNEL_BY_LOCALE[i18n.defaultLanguage];
}

// 外部 widget 脚本（frontend 构建的 rwiki-chat.js）暴露的全局 init 选项。
// 字段与 configuration 文档的 Widget 配置一致。
export type RWikiChatOptions = {
  apiUrl: string;
  channelId: string | string[];
  locale?: string;
  title?: string;
  primaryColor?: string;
  position?: "left" | "right";
  /** 面板显示形态：弹窗（默认）或右侧推挤式停靠侧栏。 */
  displayMode?: "modal" | "sidePanel";
  welcomeMessage?: string;
  messages?: Record<string, string>;
  suggestedQuestions?: string[] | Record<string, string[]>;
};

// window.RWikiChat 全局对象的唯一类型声明：搜索与聊天 widget 共用，
// 避免 TS 对该全局无声明时各组件自持副本、漂移无法在编译期暴露。
export type RWikiChatGlobal = {
  init?: (opts: RWikiChatOptions) => void;
  destroy?: () => void;
  setLocale?: (locale: string) => void;
  setChannel?: (channelId: string | string[]) => void;
  ask?: (query: string) => void;
};

/** 读取 window.RWikiChat（统一做强转，调用方不再各自 `as unknown as`）。 */
export function getRWikiChat(): RWikiChatGlobal | undefined {
  return (window as unknown as { RWikiChat?: RWikiChatGlobal }).RWikiChat;
}

/** widget 脚本就绪（window.RWikiChat 可用）后派发；消费者据此重算依赖全局 API 的入口。 */
export const RWIKI_CHAT_READY_EVENT = "rwiki-chat:ready";
