import { createFileRoute } from "@tanstack/react-router";
import { HomePage } from "@/components/home-page";
import { zh } from "@/lib/home-texts";

export const Route = createFileRoute("/$lang/")({
  component: LangHome,
});

function LangHome() {
  const { lang } = Route.useParams();

  return (
    <HomePage
      texts={zh}
      docsLink={{ to: "/$lang/docs/$", params: { lang, _splat: "getting-started" } }}
    />
  );
}
