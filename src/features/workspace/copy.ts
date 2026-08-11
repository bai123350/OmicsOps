export type Locale = "zh-CN" | "en-US";

export const copy = {
  "zh-CN": {
    projects: "项目与会话", research: "科研对话", context: "项目上下文",
    files: "文件", preview: "预览", notebook: "实验记录", explore: "探索", runs: "运行",
    expand: "展开预览", artifactPreview: "产物预览", overview: "UMAP 聚类概览",
    newConversation: "新建会话", settings: "设置", composer: "描述研究目标，或 @ 引用项目文件…",
    send: "发送", status: "远端分析运行中", task: "Scanpy 质量控制",
  },
  "en-US": {
    projects: "Projects and sessions", research: "Research conversation", context: "Project context",
    files: "Files", preview: "Preview", notebook: "Lab notebook", explore: "Explore", runs: "Runs",
    expand: "Expand preview", artifactPreview: "Artifact preview", overview: "UMAP clustering overview",
    newConversation: "New conversation", settings: "Settings", composer: "Describe a research goal or @ mention a project file…",
    send: "Send", status: "Remote analysis running", task: "Scanpy quality control",
  },
} as const;
