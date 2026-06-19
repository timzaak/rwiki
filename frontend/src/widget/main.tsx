import { createRoot } from 'react-dom/client';

import widgetStyles from './styles.css?inline';
import highlightStyles from 'highlight.js/styles/github.css?inline';
import { WidgetApp } from './widget-app';
import { validateWidgetConfig, type ValidatedWidgetConfig, type WidgetConfig } from './config';
import { injectStyles } from './inject-styles';
import { resolveLocale } from '@/components/chat/messages';
import { useChatModalStore, useChatStore } from '@/stores/chat-store';

let container: HTMLDivElement | null = null;
let shadowRoot: ShadowRoot | null = null;
let reactRoot: ReturnType<typeof createRoot> | null = null;
let currentConfig: ValidatedWidgetConfig | null = null;

function init(config: WidgetConfig) {
  const validated = validateWidgetConfig(config);
  if (!validated) return; // validateWidgetConfig logs error

  // Destroy existing instance if present
  if (container) destroy();

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
  reactRoot.render(<WidgetApp config={currentConfig} />);
}

// Expose global API
(window as any).RWikiChat = { init, destroy, setLocale };
