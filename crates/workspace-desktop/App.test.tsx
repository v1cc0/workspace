import { expect, open, test } from "nsh/test";

test("Chinese Markdown edits reach the native preview", async () => {
  const app = await open(".", { width: 1100, height: 760, platform: "linux" });
  await app.clear("id:markdown-editor");
  await app.type("id:markdown-editor", "# 新笔记\n\n中文实时预览\n\n| 列名 |\n| --- |\n| 内容 |\n\n~~已删除~~\n\n```rust\nlet value = 1;\n```\n");
  await app.waitFor("id:markdown-preview >> text:新笔记");
  expect(app.texts()).toContain("中文实时预览");
  expect(app.texts()).toContain("列名");
  expect(app.texts()).toContain("已删除");
  await app.clear("id:markdown-editor");
  expect(app.texts()).not.toContain("中文实时预览");
  await app.type("id:markdown-editor", "重新编辑");
  await app.waitFor("id:markdown-preview >> text:重新编辑");
});

test("long drafts remain editable with reduced motion in a small dark window", async () => {
  const app = await open(".", { width: 800, height: 600, platform: "linux", colorScheme: "dark", reducedMotion: true });
  const draft = "中文长段落与换行\n\n".repeat(80);
  await app.clear("label:Markdown 编辑器");
  await app.type("label:Markdown 编辑器", draft);
  expect(app.value("id:markdown-editor")).toBe(draft);
  await app.scroll("label:Markdown 预览", 2000);
  await app.clear("id:markdown-editor");
  await app.type("id:markdown-editor", "长文后继续编辑");
  await app.waitFor("id:markdown-preview >> text:长文后继续编辑");
});
