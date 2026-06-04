import { docs } from "collections/server";
import { loader, multiple } from "fumadocs-core/source";
import { lucideIconsPlugin } from "fumadocs-core/source/lucide-icons";
import { openapiPlugin, openapiSource } from "fumadocs-openapi/server";
import { i18n } from "./i18n";
import { openapi } from "./openapi";

const docsRoute = "/docs";

export const source = loader(
  multiple({
    docs: docs.toFumadocsSource(),
    openapi: await openapiSource(openapi, {
      baseDir: "openapi",
    }),
  }),
  {
    baseUrl: docsRoute,
    i18n,
    url(slugs, locale) {
      const loc = locale || i18n.defaultLanguage;
      return `/${[loc, "docs", ...slugs.filter(Boolean)].join("/")}`;
    },
    plugins: [lucideIconsPlugin(), openapiPlugin()],
  },
);

export function markdownPathToSlugs(segs: string[]) {
  if (segs.length === 0) return [];

  const out = [...segs];
  out[out.length - 1] = out[out.length - 1].replace(/\.md$/, "");
  if (out.length === 1 && out[0] === "index") out.pop();
  return out;
}

export function slugsToMarkdownPath(slugs: string[], locale?: string) {
  const segments = [...slugs];
  if (segments.length === 0) {
    segments.push("index.md");
  } else {
    segments[segments.length - 1] += ".md";
  }

  const base = locale ? `/${locale}${docsRoute}` : docsRoute;
  return {
    segments,
    url: `${base}/${segments.join("/")}`,
  };
}

export function getPageMarkdownUrl(slugs: string[]) {
  const segments = [...slugs];
  if (segments.length === 0) {
    segments.push("index.md");
  } else {
    segments[segments.length - 1] += ".md";
  }

  return {
    segments,
    url: `${docsRoute}/${segments.join("/")}`,
  };
}

export async function getLLMText(page: (typeof source)["$inferPage"]) {
  if (page.data.type === "openapi") {
    const schema = page.data.getSchema();
    return JSON.stringify(schema, null, 2);
  }

  const processed = await page.data.getText("processed");

  return `# ${page.data.title} (${page.url})

${processed}`;
}
