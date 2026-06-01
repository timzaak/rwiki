/**
 * Inject CSS into Shadow DOM with primaryColor override.
 *
 * Replaces --primary and --color-primary CSS custom property values
 * in the compiled CSS string. This works regardless of the color format
 * (oklch, hex, hsl) used in the source theme.
 */
export function injectStyles(
  shadowRoot: ShadowRoot,
  cssText: string,
  primaryColor: string,
): void {
  cssText = cssText.replace(
    /--primary\s*:\s*[^;}]+/g,
    `--primary: ${primaryColor}`,
  );
  cssText = cssText.replace(
    /--color-primary\s*:\s*[^;}]+/g,
    `--color-primary: ${primaryColor}`,
  );

  const style = document.createElement('style');
  style.textContent = cssText;
  shadowRoot.appendChild(style);
}
