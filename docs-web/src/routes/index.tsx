import { createFileRoute, Link } from "@tanstack/react-router";
import { HomeLayout } from "fumadocs-ui/layouts/home";
import { baseOptions } from "@/lib/layout.shared";
import { gitConfig } from "@/lib/shared";
import {
  Activity,
  Database,
  Code,
  FileStack,
  Sparkles,
  Cloud,
  UploadCloud,
  FileText,
  MessageSquare,
} from "lucide-react";

export const Route = createFileRoute("/")({
  component: Home,
});

const GITHUB_URL = `https://github.com/${gitConfig.user}/${gitConfig.repo}`;

function GitHubIcon({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" className={className} fill="currentColor">
      <path d="M12 2C6.477 2 2 6.484 2 12.017c0 4.425 2.865 8.18 6.839 9.504.5.092.682-.217.682-.483 0-.237-.008-.868-.013-1.703-2.782.605-3.369-1.343-3.369-1.343-.454-1.158-1.11-1.466-1.11-1.466-.908-.62.069-.608.069-.608 1.003.07 1.531 1.032 1.531 1.032.892 1.53 2.341 1.088 2.91.832.092-.647.35-1.088.636-1.338-2.22-.253-4.555-1.113-4.555-4.951 0-1.093.39-1.988 1.029-2.688-.103-.253-.446-1.272.098-2.65 0 0 .84-.27 2.75 1.026A9.564 9.564 0 0112 6.844c.85.004 1.705.115 2.504.337 1.909-1.296 2.747-1.027 2.747-1.027.546 1.379.202 2.398.1 2.651.64.7 1.028 1.595 1.028 2.688 0 3.848-2.339 4.695-4.566 4.943.359.309.678.92.678 1.855 0 1.338-.012 2.419-.012 2.747 0 .268.18.58.688.482A10.019 10.019 0 0022 12.017C22 6.484 17.522 2 12 2z" />
    </svg>
  );
}

function Home() {
  return (
    <HomeLayout {...baseOptions()}>
      <div className="relative overflow-hidden selection:bg-purple-200">
        {/* Background Gradient Blobs */}
        <div className="absolute top-[-10%] left-[-10%] w-[50vw] h-[50vw] max-w-[600px] max-h-[600px] bg-purple-200/50 rounded-full mix-blend-multiply filter blur-[100px] opacity-70 animate-pulse pointer-events-none" />
        <div className="absolute top-[-5%] right-[-5%] w-[45vw] h-[45vw] max-w-[500px] max-h-[500px] bg-blue-200/50 rounded-full mix-blend-multiply filter blur-[100px] opacity-60 animate-pulse pointer-events-none" />

        {/* Hero Section */}
        <section className="relative z-10 pt-24 pb-16 px-4">
          <div className="max-w-4xl mx-auto text-center">
            <h1 className="text-5xl md:text-7xl font-serif font-bold text-gray-900 leading-[1.1] mb-6">
              Self-hosted
              <br />
              knowledge base Q&A.
            </h1>
            <p className="text-lg md:text-xl text-gray-600 mb-10 max-w-2xl mx-auto leading-relaxed">
              Upload your docs, ask questions, get sourced answers.
              <br className="hidden md:block" />
              Single binary, SQLite, zero external databases.
            </p>

            <div className="flex flex-col sm:flex-row items-center justify-center gap-4 flex-wrap">
              <a
                href={GITHUB_URL}
                target="_blank"
                rel="noopener noreferrer"
                className="flex items-center gap-2 bg-[#8b5cf6] hover:bg-[#7c3aed] text-white px-8 py-3.5 rounded-full font-medium transition-all shadow-lg shadow-purple-200 justify-center"
              >
                <GitHubIcon className="w-5 h-5" />
                Star on GitHub
              </a>
              <Link
                to="/docs/$"
                params={{ _splat: "getting-started" }}
                className="flex items-center gap-2 bg-white/60 hover:bg-white backdrop-blur-md text-gray-700 px-8 py-3.5 rounded-full border border-gray-200/80 font-medium transition-all shadow-sm justify-center"
              >
                Read Docs
              </Link>
            </div>
          </div>

          {/* Terminal/Code Snippet */}
          <div className="relative mt-20 max-w-2xl mx-auto">
            <div className="absolute inset-0 bg-gradient-to-r from-blue-400 via-indigo-400 to-purple-400 blur-2xl opacity-20 md:opacity-30 rounded-3xl transform scale-y-75 translate-y-4" />

            <div className="relative bg-white/70 backdrop-blur-xl border border-white/60 shadow-[0_8px_30px_rgb(0,0,0,0.06)] rounded-2xl p-2 z-10 overflow-hidden">
              <div className="flex gap-2 px-3 py-2">
                <div className="w-3 h-3 rounded-full bg-[#ff5f56]" />
                <div className="w-3 h-3 rounded-full bg-[#ffbd2e]" />
                <div className="w-3 h-3 rounded-full bg-[#27c93f]" />
              </div>
              <div className="px-6 py-5 md:py-6 bg-white/50 rounded-xl font-mono text-sm md:text-base text-gray-800 break-all sm:break-normal text-center sm:text-left overflow-x-auto shadow-inner">
                <span className="text-pink-400 select-none mr-2 font-bold">
                  $
                </span>
                docker run -d -p 8080:8080 -v rwiki-data:/app/data rwiki
              </div>
            </div>
          </div>
        </section>

        {/* Feature Grid */}
        <section className="relative z-10 mt-16 max-w-4xl mx-auto px-4 pb-20">
          <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 gap-6 cursor-default">
            <FeatureCard
              icon={<Activity className="w-6 h-6 text-gray-700" />}
              title="Streaming Q&A"
            />
            <FeatureCard
              icon={<Database className="w-6 h-6 text-gray-700" />}
              title="SQLite only"
            />
            <FeatureCard
              icon={<Code className="w-6 h-6 text-gray-700" />}
              title="Embeddable widget"
            />
            <FeatureCard
              icon={<FileStack className="w-6 h-6 text-gray-700" />}
              title="Multi-format"
            />
            <FeatureCard
              icon={<Sparkles className="w-6 h-6 text-gray-700" />}
              title="Any LLM provider"
            />
            <FeatureCard
              icon={<Cloud className="w-6 h-6 text-gray-700" />}
              title="Self-hosted"
            />
          </div>
        </section>

        {/* How it works */}
        <section className="relative z-10 py-24 px-4 bg-gradient-to-b from-transparent to-white/40">
          <div className="max-w-xl mx-auto">
            <h2 className="text-3xl md:text-4xl font-serif font-bold text-gray-900 mb-16 text-center">
              How it works
            </h2>

            <div className="relative space-y-12">
              <div className="absolute top-8 left-[3.35rem] bottom-8 w-px bg-gray-200 hidden sm:block pointer-events-none" />

              <Step
                icon={
                  <UploadCloud
                    className="w-8 h-8 text-gray-700"
                    strokeWidth={1.5}
                  />
                }
                num="1"
                title="Upload"
                desc="Upload your docs, files, and links directly into the system for secure local processing."
              />
              <Step
                icon={
                  <FileText
                    className="w-8 h-8 text-gray-700"
                    strokeWidth={1.5}
                  />
                }
                num="2"
                title="Publish"
                desc="Content is processed, indexed, and optimally stored locally without external databases."
              />
              <Step
                icon={
                  <MessageSquare
                    className="w-8 h-8 text-gray-700"
                    strokeWidth={1.5}
                  />
                }
                num="3"
                title="Ask"
                desc="Ask questions and get instant, context-aware answers directly supported by your data."
              />
            </div>
          </div>
        </section>

        {/* Why RWiki Comparison Table */}
        <section className="relative z-10 py-24 px-4">
          <div className="max-w-4xl mx-auto">
            <h2 className="text-3xl md:text-4xl font-serif font-bold text-gray-900 mb-12 text-center">
              Why RWiki
            </h2>

            <div className="bg-white/80 backdrop-blur-lg rounded-3xl shadow-[0_8px_40px_rgb(0,0,0,0.04)] border border-gray-100 overflow-hidden">
              <div className="overflow-x-auto">
                <table className="w-full text-left border-collapse min-w-[600px]">
                  <thead>
                    <tr className="border-b border-gray-100">
                      <th className="px-8 py-6 font-medium text-gray-400 w-1/3" />
                      <th className="px-8 py-6 font-bold text-gray-900 w-1/3 text-lg">
                        RWiki
                      </th>
                      <th className="px-8 py-6 font-bold text-gray-900 w-1/3 text-lg">
                        Typical RAG
                      </th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-gray-50 text-gray-600">
                    <TableRow
                      label="Database"
                      rwiki="SQLite"
                      typical="Vector DB + PostGres"
                    />
                    <TableRow
                      label="Dependencies"
                      rwiki="None"
                      typical="Multiple containers"
                    />
                    <TableRow
                      label="Deployment"
                      rwiki="Single binary, drop-in"
                      typical="Complex setup scripts"
                    />
                    <TableRow
                      label="Setup"
                      rwiki="5 minutes"
                      typical="Requires expertise"
                    />
                  </tbody>
                </table>
              </div>
            </div>
          </div>
        </section>

        {/* Bottom CTA */}
        <section className="relative z-10 py-20 px-4 text-center">
          <h2 className="text-3xl md:text-4xl font-serif font-bold text-gray-900 mb-2">
            Open source, Apache 2.0.
          </h2>
          <h2 className="text-3xl md:text-4xl font-serif font-bold text-gray-900 mb-10">
            Try it in 5 minutes.
          </h2>

          <div className="flex flex-col sm:flex-row items-center justify-center gap-4 flex-wrap">
            <a
              href={GITHUB_URL}
              target="_blank"
              rel="noopener noreferrer"
              className="flex items-center gap-2 bg-[#8b5cf6] hover:bg-[#7c3aed] text-white px-8 py-3.5 rounded-full font-medium transition-all shadow-lg shadow-purple-200 justify-center"
            >
              <GitHubIcon className="w-5 h-5" />
              Star on GitHub
            </a>
            <Link
              to="/docs/$"
              params={{ _splat: "getting-started" }}
              className="flex items-center gap-2 bg-white/60 hover:bg-white backdrop-blur-md text-gray-700 px-8 py-3.5 rounded-full border border-gray-200/80 font-medium transition-all shadow-sm justify-center"
            >
              Get Started
            </Link>
          </div>
        </section>
      </div>
    </HomeLayout>
  );
}

function FeatureCard({
  icon,
  title,
}: {
  icon: React.ReactNode;
  title: string;
}) {
  return (
    <div className="bg-white/70 backdrop-blur-sm rounded-2xl py-8 px-4 flex flex-col items-center gap-4 text-center shadow-[0_4px_20px_rgb(0,0,0,0.03)] border border-white hover:shadow-[0_8px_30px_rgb(0,0,0,0.06)] hover:bg-white transition-all group">
      <div className="bg-gray-50 border border-gray-100 p-4 rounded-xl group-hover:scale-110 group-hover:shadow-sm transition-all duration-300">
        {icon}
      </div>
      <h3 className="font-semibold text-gray-900 tracking-tight">{title}</h3>
    </div>
  );
}

function Step({
  icon,
  num,
  title,
  desc,
}: {
  icon: React.ReactNode;
  num: string;
  title: string;
  desc: string;
}) {
  return (
    <div className="flex flex-col sm:flex-row items-center sm:items-start gap-4 sm:gap-8 relative z-10 group">
      <div className="bg-white border text-gray-700 border-gray-100 p-4 rounded-xl shadow-sm relative z-10 group-hover:border-purple-200 transition-colors">
        {icon}
      </div>

      <div className="hidden sm:flex w-6 h-6 rounded-full bg-white border border-gray-200 items-center justify-center text-xs font-semibold text-gray-500 relative mt-5 shrink-0 z-10 group-hover:border-purple-300 group-hover:text-purple-600 transition-colors">
        {num}
      </div>

      <div className="flex-1 text-center sm:text-left mt-2 sm:mt-4">
        <h3 className="text-xl font-bold text-gray-900 mb-2 font-serif tracking-tight">
          {title}
        </h3>
        <p className="text-gray-600 leading-relaxed text-sm max-w-sm mx-auto sm:mx-0">
          {desc}
        </p>
      </div>
    </div>
  );
}

function TableRow({
  label,
  rwiki,
  typical,
}: {
  label: string;
  rwiki: string;
  typical: string;
}) {
  return (
    <tr className="hover:bg-gray-50/50 transition-colors">
      <td className="px-8 py-5 font-semibold text-gray-900 border-r border-gray-50">
        {label}
      </td>
      <td className="px-8 py-5 text-gray-800 font-medium">{rwiki}</td>
      <td className="px-8 py-5 text-gray-500">{typical}</td>
    </tr>
  );
}
