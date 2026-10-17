import { describe, expect, it } from "vitest";
import { chatFilePresentation } from "./file-icon-theme";
import { CHAT_FILE_ICON_IDS, chatFileIconUrl } from "./file-icons.generated";

const iconSources = import.meta.glob<string>(
  "../../../../static/file-icons/icons/*.svg",
  { eager: true, query: "?raw", import: "default" },
);

describe("Chat file icon theme", () => {
  it("uses recognizable framework and language icons", () => {
    expect(chatFilePresentation("src/App.svelte").icon).toBe("svelte");
    expect(chatFilePresentation("src/Button.tsx").icon).toBe("react");
    expect(chatFilePresentation("src/main.rs")).toEqual({ icon: "rust", darkIcon: "rust-dark" });
    expect(chatFilePresentation("lib/example.py").icon).toBe("python");
    expect(chatFilePresentation("main.go").icon).toBe("go");
  });

  it("prioritizes exact names, configuration stems, and compound suffixes", () => {
    expect(chatFilePresentation("package.json").icon).toBe("node");
    expect(chatFilePresentation("PNPM-LOCK.YAML").icon).toBe("pnpm");
    expect(chatFilePresentation("configs/tsconfig.app.json").icon).toBe("typescript-config");
    expect(chatFilePresentation("vite.config.ts").icon).toBe("vite");
    expect(chatFilePresentation("src/model.d.ts").icon).toBe("typescript-definition");
    expect(chatFilePresentation("src/model.test.ts").icon).toBe("typescript-test");
    expect(chatFilePresentation("src/view.spec.tsx").icon).toBe("react");
  });

  it("covers common tooling, infrastructure, data, media, and fallback files", () => {
    expect(chatFilePresentation("Dockerfile").icon).toBe("docker");
    expect(chatFilePresentation("BUILD.bazel").icon).toBe("bazel");
    expect(chatFilePresentation("Jenkinsfile").icon).toBe("jenkins");
    expect(chatFilePresentation("terraform/main.tf").icon).toBe("terraform");
    expect(chatFilePresentation("schema.prisma").icon).toBe("prisma");
    expect(chatFilePresentation("docs/guide.pdf").icon).toBe("pdf");
    expect(chatFilePresentation("reports/budget.xlsx").icon).toBe("excel");
    expect(chatFilePresentation("reports/demo.pptx").icon).toBe("powerpoint");
    expect(chatFilePresentation("assets/logo.svg").icon).toBe("svg");
    expect(chatFilePresentation("artifact.unknown-extension").icon).toBe("default");
  });

  it("recognizes additional common language and template formats", () => {
    expect(chatFilePresentation("legacy/main.cob").icon).toBe("cobol");
    expect(chatFilePresentation("science/model.f90").icon).toBe("fortran");
    expect(chatFilePresentation("frontend/theme.styl").icon).toBe("stylus");
    expect(chatFilePresentation("templates/page.njk").icon).toBe("jinja");
  });

  it("resolves icons through standalone Vite asset URLs", () => {
    expect(chatFileIconUrl("svelte")).toMatch(/svelte\.svg(?:\?no-inline)?$/);
    expect(chatFileIconUrl("svelte")).not.toContain("#");
  });

  it("ships every generated local image without active SVG content", () => {
    for (const iconId of CHAT_FILE_ICON_IDS) {
      const icon = iconSources[`../../../../static/file-icons/icons/${iconId}.svg`];
      expect(icon).toBeTypeOf("string");
      if (typeof icon !== "string") continue;
      expect(icon).toMatch(/^<svg\b/);
      expect(icon).not.toMatch(/<(?:script|foreignObject|iframe|object|embed)\b/i);
      expect(icon).not.toMatch(/\son[a-z]+\s*=/i);
      expect(icon).not.toMatch(/\b(?:href|xlink:href)=["'](?!#)/i);
    }
  });
});
