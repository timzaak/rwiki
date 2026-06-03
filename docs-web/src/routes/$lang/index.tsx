import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { useEffect } from "react";
import { i18n } from "@/lib/i18n";

export const Route = createFileRoute("/$lang/")({
  component: LangIndexRedirect,
});

function LangIndexRedirect() {
  const { lang } = Route.useParams();
  const navigate = useNavigate();

  useEffect(() => {
    if (lang === i18n.defaultLanguage) {
      void navigate({
        to: "/docs/$",
        params: { _splat: "getting-started" },
      });
    } else {
      void navigate({
        to: "/$lang/docs/$",
        params: { lang, _splat: "getting-started" },
      });
    }
  }, [lang, navigate]);

  return null;
}
