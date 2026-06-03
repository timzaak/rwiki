import { createFileRoute } from "@tanstack/react-router";
import { HomePage } from "@/components/home-page";
import { en } from "@/lib/home-texts";

export const Route = createFileRoute("/")({
  component: () => (
    <HomePage
      texts={en}
      docsLink={{ to: "/docs/$", params: { _splat: "getting-started" } }}
    />
  ),
});
