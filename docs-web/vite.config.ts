import tailwindcss from "@tailwindcss/vite";
import { tanstackStart } from "@tanstack/react-start/plugin/vite";
import react from "@vitejs/plugin-react";
import mdx from "fumadocs-mdx/vite";
import { nitro } from "nitro/vite";
import { defineConfig } from "vite";

const OPENAPI_SLUGS = ["chat", "delete_document", "health_check", "list_documents", "publish_document", "unpublish_document", "upload_document"];

function openapiPages() {
  return OPENAPI_SLUGS.flatMap((slug) => [
    { path: `/docs/openapi/${slug}` },
    { path: `/en/docs/openapi/${slug}` },
    { path: `/zh/docs/openapi/${slug}` },
  ]);
}

export default defineConfig({
  define: {
    "import.meta.env.VITE_RWIKI_API_URL": JSON.stringify(
      process.env.VITE_RWIKI_API_URL || "http://localhost:18080",
    ),
  },
  server: {
    port: 3001,
  },
  plugins: [
    mdx(),
    tailwindcss(),
    tanstackStart({
      spa: {
        enabled: true,
        prerender: {
          enabled: true,
          crawlLinks: true,
        },
      },

      pages: [
        {
          path: "/docs",
        },
        {
          path: "/zh/docs",
        },
        // Locale root pages (client-side redirect to docs)
        {
          path: "/zh",
        },
        {
          path: "/en",
        },
        // OpenAPI pages (crawler doesn't discover these from virtual pages)
        ...openapiPages(),
        {
          path: "/api/search",
        },
        {
          path: "llms-full.txt",
        },
        {
          path: "llms.txt",
        },
      ],
    }),
    react(),
    // please see https://tanstack.com/start/latest/docs/framework/react/guide/hosting#nitro for guides on hosting
    nitro(),
  ],
  resolve: {
    tsconfigPaths: true,
  },
});
