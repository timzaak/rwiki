import {
  createFileRoute,
  redirect,
} from "@tanstack/react-router";

export const Route = createFileRoute("/docs/$")({
  beforeLoad: ({ params }) => {
    const slugs = params._splat?.split("/").filter(Boolean) ?? [];
    throw redirect({ href: `/en/docs/${slugs.join("/")}` });
  },
});
