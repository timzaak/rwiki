import { flushSync } from 'react-dom';
import { createRoot } from 'react-dom/client';

import widgetStyles from './styles.css?inline';
import highlightStyles from 'highlight.js/styles/github.css?inline';
import { WidgetApp } from './widget-app';
import { normalizeChannelIds, validateWidgetConfig, type ValidatedWidgetConfig, type WidgetConfig } from './config';
import { injectStyles } from './inject-styles';
import { removeSidePanelHostInjection, setSidePanelOpen } from './side-panel-host';
import { stopWidgetStreaming } from './use-widget-chat-stream';
import { resolveLocale } from '@/components/chat/messages';
import { useChatModalStore, useChatStore } from '@/stores/chat-store';
import {
  archiveCurrentSession,
  rebindWorkspaceChannel,
} from '@/stores/chat-history-store';
import { channelKeyOf } from '@/lib/channel-key';

let container: HTMLDivElement | null = null;
let shadowRoot: ShadowRoot | null = null;
let reactRoot: ReturnType<typeof createRoot> | null = null;
let currentConfig: ValidatedWidgetConfig | null = null;
let unsubscribeSidePanelHost: (() => void) | null = null;
let darkModeObserver: MutationObserver | null = null;

// A conversation belongs to its channel: rebinding archives the current session
// into its own channel's history, then clears and rebinds the workspace (see
// rebindWorkspaceChannel in chat-history-store) so a persisted session from a
// different channel (e.g. another language's site) never rehydrates under the
// new channel. No-op for an unchanged key.
function bindChannel(channelIds: readonly string[]) {
  rebindWorkspaceChannel(channelKeyOf(channelIds));
}

function init(config: WidgetConfig) {
  const validated = validateWidgetConfig(config);
  if (!validated) return; // validateWidgetConfig logs error

  // Destroy existing instance if present
  if (container) destroy();

  // Bind the channel before mounting: drop any persisted session that belongs
  // to a different channel (destroy() above may already have reset it).
  bindChannel(validated.channelId);

  // Side panel mode: drive the host-page layout shift from the shared open
  // state, so ask()/floating button/close icon all route through it unchanged.
  // The modal mode never subscribes — host DOM footprint stays as-is.
  if (validated.displayMode === 'sidePanel') {
    unsubscribeSidePanelHost = useChatModalStore.subscribe((state, prev) => {
      if (state.isModalOpen === prev.isModalOpen) return;
      setSidePanelOpen(state.isModalOpen);
    });
  }

  // Create host container
  container = document.createElement('div');
  container.id = 'rwiki-chat-widget';

  // Mirror dark mode from host page to Shadow DOM host element
  const updateDark = () => {
    const isDark = document.documentElement.classList.contains('dark');
    container!.classList.toggle('dark', isDark);
  };
  updateDark();
  darkModeObserver = new MutationObserver(updateDark);
  darkModeObserver.observe(document.documentElement, {
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

  // Detach the host-page layout shift synchronously (no animation wait) before
  // the store resets below fire any subscription callback. No-op in modal mode.
  unsubscribeSidePanelHost?.();
  unsubscribeSidePanelHost = null;
  removeSidePanelHostInjection();

  // Stop observing the host page: a live observer after destroy would crash on
  // the next host class change (container is gone) — and is host residue.
  darkModeObserver?.disconnect();
  darkModeObserver = null;

  // Unmount React
  reactRoot?.unmount();
  reactRoot = null;

  // Reset stores. The current session is archived first (its channel's
  // history survives destroy — the rwiki-chat-history key is never touched
  // here); the workspace key is cleared with the store.
  useChatModalStore.getState().closeModal();
  const fallbackChannelKey = currentConfig
    ? channelKeyOf(currentConfig.channelId)
    : undefined;
  archiveCurrentSession(fallbackChannelKey);
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
// new conversation: abort the in-flight answer, archive the old conversation
// into its channel's history, clear the workspace and re-render with the new
// channel (bindChannel). No-op for an unchanged channel set (would otherwise
// wipe the conversation).
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
