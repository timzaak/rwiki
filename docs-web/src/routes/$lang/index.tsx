import { createFileRoute } from "@tanstack/react-router";
import { HomePage } from "@/components/home-page";
import { en, type HomeTexts, zh } from "@/lib/home-texts";
import {
  canonicalLink,
  homeJsonLd,
  homeTitles,
  seoMeta,
  siteDescriptions,
} from "@/lib/seo";

const textsMap: Record<string, HomeTexts> = { en, zh };

export const Route = createFileRoute("/$lang/")({
  component: LangHome,
  head: ({ params }) => {
    const lang = textsMap[params.lang] ? params.lang : "en";
    return {
      meta: seoMeta({
        title: homeTitles[lang] ?? homeTitles.en,
        description: siteDescriptions[lang] ?? siteDescriptions.en,
        path: `/${lang}`,
        locale: lang,
      }),
      links: [canonicalLink(`/${lang}`)],
      scripts: [
        {
          type: "application/ld+json",
          children: JSON.stringify(homeJsonLd(lang)),
        },
      ],
    };
  },
});

function LangHome() {
  const { lang } = Route.useParams();
  const texts = textsMap[lang] ?? en;

  return (
    <HomePage
      lang={lang}
      texts={texts}
      docsLink={{
        to: "/$lang/docs/$",
        params: { lang, _splat: "getting-started" },
      }}
    />
  );
}
