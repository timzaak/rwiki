// Single source for the SEO head signals — title/description/og/canonical.
//
// Most directory-site crawlers, search engines, and AI assistants read only
// the prerendered <head>; they never run the SPA. These tags are the site's
// only reliable identity signal, so the copy here must stay in sync with the
// approved README taglines (README.md / README.zh-CN.md).

import { appName } from "./shared";

export const siteUrl = "https://rwiki.fornetcode.com";
// 1200×675 social cards already shipped in public/ — one per language.
export const siteOgImages: Record<string, string> = {
  en: `${siteUrl}/knowledge-card-en.png`,
  zh: `${siteUrl}/knowledge-card.png`,
};
export const siteOgImage = siteOgImages.en;

export const homeTitles: Record<string, string> = {
  en: "RWiki — Self-hosted RAG Knowledge Base Q&A",
  zh: "RWiki — 自托管 RAG 知识库问答",
};

export const siteDescriptions: Record<string, string> = {
  en: "Open-source (Apache-2.0) RAG knowledge base Q&A — upload docs, get streaming answers with structured citations. Hybrid search, single binary, zero external databases.",
  zh: "开源（Apache-2.0）的 RAG 知识库问答：上传文档即问即答，回答带结构化引用。混合检索，单二进制 + SQLite，零外部数据库。",
};

export const docsDescriptions: Record<string, string> = {
  en: "RWiki documentation: quick start, configuration, deployment, and the HTTP API.",
  zh: "RWiki 文档：快速上手、配置、部署与 HTTP API。",
};

const docsTitleSuffixes: Record<string, string> = { en: "Docs", zh: "文档" };

export function docsTitle(title: string | undefined, locale: string) {
  return `${title} — ${appName} ${docsTitleSuffixes[locale] ?? docsTitleSuffixes.en}`;
}

interface SeoMetaInput {
  title: string;
  description: string;
  /** Canonical path including locale prefix, e.g. "/en/docs/getting-started". */
  path: string;
  /** Drives og:locale and the language-matched og:image card. */
  locale?: string;
}

type MetaEntry =
  | { title: string }
  | { name: string; content: string }
  | { property: string; content: string };

const ogLocales: Record<string, string> = { en: "en_US", zh: "zh_CN" };

// Root route repeats the same name/property keys with default copy; TanStack
// Router dedupes head metas by name/property with the deepest match winning,
// so leaf values here always override the root defaults.
export function seoMeta({
  title,
  description,
  path,
  locale,
}: SeoMetaInput): MetaEntry[] {
  const ogImage = (locale && siteOgImages[locale]) || siteOgImage;
  const metas: MetaEntry[] = [
    { title },
    { name: "description", content: description },
    { property: "og:title", content: title },
    { property: "og:description", content: description },
    { property: "og:url", content: `${siteUrl}${path}` },
    { property: "og:image", content: ogImage },
    { property: "og:site_name", content: appName },
    { property: "og:type", content: "website" },
    { name: "twitter:card", content: "summary_large_image" },
    { name: "twitter:title", content: title },
    { name: "twitter:description", content: description },
    { name: "twitter:image", content: ogImage },
  ];
  if (locale && ogLocales[locale]) {
    metas.push({ property: "og:locale", content: ogLocales[locale] });
  }
  return metas;
}

// Canonical links are appended (not deduped) across matched routes, so only
// leaf routes may emit one — never the root defaults.
export function canonicalLink(path: string) {
  return { rel: "canonical", href: `${siteUrl}${path}` };
}

export function homeJsonLd(locale: string) {
  return {
    "@context": "https://schema.org",
    "@type": "SoftwareApplication",
    name: appName,
    applicationCategory: "DeveloperApplication",
    operatingSystem: "Linux, Docker",
    url: siteUrl,
    image: siteOgImages[locale] ?? siteOgImage,
    description: siteDescriptions[locale] ?? siteDescriptions.en,
    license: "https://www.apache.org/licenses/LICENSE-2.0",
    offers: { "@type": "Offer", price: "0", priceCurrency: "USD" },
  };
}
