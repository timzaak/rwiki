import { flushSync } from 'react-dom';
import { createRoot } from 'react-dom/client';

import widgetStyles from './styles.css?inline';
import highlightStyles from 'highlight.js/styles/github.css?inline';
import { WidgetApp } from './widget-app';
import { normalizeChannelIds, validateWidgetConfig, type ValidatedWidgetConfig, type WidgetConfig } from './config';
import { injectStyles } from './inject-styles';
import { stopWidgetStreaming } from './use-widget-chat-stream';
import { resolveLocale } from '@/components/chat/messages';
import { useChatModalStore, useChatStore } from '@/stores/chat-store';

let container: HTMLDivElement | null = null;
let shadowRoot: ShadowRoot | null = null;
let reactRoot: ReturnType<typeof createRoot> | null = null;
let currentConfig: ValidatedWidgetConfig | null = null;

// Separator-safe encoding of a channel ID set (a plain join(',') key would
// collide for IDs containing the separator, e.g. ["a","b"] vs ["a,b"]).
function channelKeyOf(channelIds: readonly string[]): string {
  return JSON.stringify([...channelIds].sort());
}

// A conversation belongs to its channel. setChannel() clears on switch; init()
// must do the same for the full-page-load path, where a persisted session from
// a different channel (e.g. another language's site) would otherwise be
// rehydrated under the new channel.
function bindChannel(channelIds: readonly string[]) {
  const channelKey = channelKeyOf(channelIds);
  const store = useChatStore.getState();
  if (store.channelKey !== channelKey) {
    store.clearMessages();
  }
  store.setChannelKey(channelKey);
}

function init(config: WidgetConfig) {
  const validated = validateWidgetConfig(config);
  if (!validated) return; // validateWidgetConfig logs error

  // Destroy existing instance if present
  if (container) destroy();

  // Bind the channel before mounting: drop any persisted session that belongs
  // to a different channel (destroy() above may already have reset it).
  bindChannel(validated.channelId);

  // Create host container
  container = document.createElement('div');
  container.id = 'rwiki-chat-widget';

  // Mirror dark mode from host page to Shadow DOM host element
  const updateDark = () => {
    const isDark = document.documentElement.classList.contains('dark');
    container!.classList.toggle('dark', isDark);
  };
  updateDark();
  new MutationObserver(updateDark).observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['class'],
  });

  document.body.appendChild(container);

  // Attach Shadow DOM
  shadowRoot = container.attachShadow({ mode: 'open' });

  // Inject styles (Tailwind + highlight.js + primaryColor override)
  injectStyles(shadowRoot, widgetStyles + '\n' + highlightStyles, validated.primaryColor);

  // Mount React
  reactRoot = createRoot(shadowRoot);
  currentConfig = validated;
  reactRoot.render(<WidgetApp config={validated} />);
}

function destroy() {
  if (!container) return;

  // Unmount React
  reactRoot?.unmount();
  reactRoot = null;

  // Reset stores
  useChatModalStore.getState().closeModal();
  useChatStore.getState().clearMessages();
  useChatStore.getState().setPendingQuestion(null);
  useChatStore.getState().setChannelKey(null);

  // Remove DOM
  container.remove();
  container = null;
  shadowRoot = null;
  currentConfig = null;
}

// Switch the widget's language live without unmounting.
// Re-renders <WidgetApp> with an updated locale; conversation state (kept in
// the zustand chat-store, outside the React tree) is preserved.
function setLocale(locale: string) {
  if (!container || !reactRoot || !currentConfig) {
    console.error('[RWikiChat] setLocale called before init');
    return;
  }
  currentConfig = { ...currentConfig, locale: resolveLocale(locale) };
  // Commit synchronously like setChannel(): an ask() right after setLocale()
  // would otherwise render a frame with the previous locale.
  const root = reactRoot;
  const config = currentConfig;
  flushSync(() => root.render(<WidgetApp config={config} />));
}

// Hand a query over to the chat (US-INTG-017, mirrors the main site's
// "用 AI 回答"): open the modal and deliver the question as a chat turn through
// the store bridge consumed inside <WidgetApp> (works before the tree has
// mounted, too). An in-flight answer is interrupted by sendMessage's existing
// implicit-interrupt semantics.
function ask(query: string) {
  if (!container || !reactRoot || !currentConfig) {
    console.error('[RWikiChat] ask called before init');
    return;
  }
  const question = typeof query === 'string' ? query.trim() : '';
  if (!question) return; // the backend rejects empty messages
  useChatModalStore.getState().openModal();
  useChatStore.getState().setPendingQuestion(question);
}

// Switch the widget's channel at runtime (US-INTG-016). A new channel means a
// new conversation: abort the in-flight answer, clear transcript and session,
// then re-render with the new channel. No-op for an unchanged channel set
// (would otherwise wipe the conversation).
function setChannel(channelId: string | string[]) {
  if (!container || !reactRoot || !currentConfig) {
    console.error('[RWikiChat] setChannel called before init');
    return;
  }
  const normalized = normalizeChannelIds(channelId);
  if (!normalized) {
    console.error('[RWikiChat] channelId is required');
    return;
  }
  // Same channel set (the backend sorts channel ids) → keep the conversation.
  // Compare as sets directly: a join(',') key would collide for IDs containing
  // the separator (["a","b"] vs ["a,b"]).
  const current = currentConfig.channelId;
  const sameChannels =
    normalized.length === current.length &&
    [...normalized].every((id) => current.includes(id));
  if (sameChannels) return;

  stopWidgetStreaming();
  useChatStore.getState().clearMessages();
  bindChannel(normalized);
  currentConfig = { ...currentConfig, channelId: normalized };
  const root = reactRoot;
  const config = currentConfig;
  // Commit synchronously: a plain root.render() is a default-priority update,
  // so an ask() right after setChannel() (its store update renders at sync
  // priority first) would still send through the old channel.
  flushSync(() => root.render(<WidgetApp config={config} />));
}

// Expose global API
(window as any).RWikiChat = { init, destroy, setLocale, ask, setChannel };
