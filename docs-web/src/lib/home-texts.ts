export interface HomeTexts {
  badge: string;
  heroTitleBefore: string;
  heroTitleEm: string;
  heroDesc1: string;
  heroDesc2: string;
  starGithub: string;
  readDocs: string;
  featureTitle: string;
  featureDesc: string;
  features: { title: string; desc: string }[];
  howTitle: string;
  howDesc: string;
  steps: { title: string; desc: string }[];
  whyTitle: string;
  whyDesc: string;
  tableRows: { label: string; rwiki: string; typical: string }[];
  ctaTitle: string;
  ctaDesc: string;
  getStarted: string;
}

export const en: HomeTexts = {
  badge: "Open source · Apache 2.0",
  heroTitleBefore: "Your docs,",
  heroTitleEm: "answered.",
  heroDesc1: "Upload your docs, ask questions, get sourced answers.",
  heroDesc2: "Single binary, SQLite, zero external databases.",
  starGithub: "Star on GitHub",
  readDocs: "Read Docs",
  featureTitle: "Everything you need",
  featureDesc: "Built for simplicity, designed for power.",
  features: [
    { title: "Streaming Q&A", desc: "Real-time streaming responses" },
    { title: "SQLite only", desc: "No external database needed" },
    { title: "Embeddable widget", desc: "Drop into any website" },
    { title: "Multi-format", desc: "PDF, Markdown, DOCX, URLs" },
    { title: "Any LLM provider", desc: "OpenAI, Claude, Gemini, local" },
    { title: "Self-hosted", desc: "Your data, your infrastructure" },
  ],
  howTitle: "How it works",
  howDesc: "Three steps to your own knowledge base.",
  steps: [
    {
      title: "Upload",
      desc: "Upload your docs, files, and links directly into the system for secure local processing.",
    },
    {
      title: "Publish",
      desc: "Content is processed, indexed, and optimally stored locally without external databases.",
    },
    {
      title: "Ask",
      desc: "Ask questions and get instant, context-aware answers directly supported by your data.",
    },
  ],
  whyTitle: "Why RWiki",
  whyDesc: "Compared to typical RAG solutions.",
  tableRows: [
    { label: "Database", rwiki: "SQLite", typical: "Vector DB + Postgres" },
    { label: "Dependencies", rwiki: "None", typical: "Multiple containers" },
    { label: "Deployment", rwiki: "Single binary, drop-in", typical: "Complex setup scripts" },
    { label: "Setup", rwiki: "5 minutes", typical: "Requires expertise" },
  ],
  ctaTitle: "Open source, Apache 2.0.",
  ctaDesc: "Try it in 5 minutes.",
  getStarted: "Get Started",
};

export const zh: HomeTexts = {
  badge: "开源 · Apache 2.0",
  heroTitleBefore: "你的文档，",
  heroTitleEm: "有问必答。",
  heroDesc1: "上传文档，提问并获得带来源的答案。",
  heroDesc2: "单二进制文件，SQLite，零外部数据库。",
  starGithub: "Star on GitHub",
  readDocs: "阅读文档",
  featureTitle: "你所需的一切",
  featureDesc: "简洁至上，强大在内。",
  features: [
    { title: "流式问答", desc: "实时流式响应" },
    { title: "仅需 SQLite", desc: "无需外部数据库" },
    { title: "可嵌入组件", desc: "嵌入任意网站" },
    { title: "多格式支持", desc: "PDF、Markdown、DOCX、URL" },
    { title: "任意 LLM 提供商", desc: "OpenAI、Claude、Gemini、本地模型" },
    { title: "自托管", desc: "你的数据，你的基础设施" },
  ],
  howTitle: "工作原理",
  howDesc: "三步搭建你的专属知识库。",
  steps: [
    {
      title: "上传",
      desc: "上传文档、文件和链接，系统将在本地安全处理。",
    },
    {
      title: "发布",
      desc: "内容被处理、索引并优化存储，无需外部数据库。",
    },
    {
      title: "提问",
      desc: "提出问题，即时获得由你的数据支撑的、具备上下文感知的答案。",
    },
  ],
  whyTitle: "为什么选择 RWiki",
  whyDesc: "与典型 RAG 方案对比。",
  tableRows: [
    { label: "数据库", rwiki: "SQLite", typical: "向量数据库 + Postgres" },
    { label: "依赖", rwiki: "无", typical: "多个容器" },
    { label: "部署", rwiki: "单二进制，开箱即用", typical: "复杂安装脚本" },
    { label: "配置", rwiki: "5 分钟", typical: "需要专业知识" },
  ],
  ctaTitle: "开源，Apache 2.0 协议。",
  ctaDesc: "5 分钟即可体验。",
  getStarted: "快速开始",
};
