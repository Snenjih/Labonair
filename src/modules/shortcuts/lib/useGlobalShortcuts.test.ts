import { describe, expect, it } from "vitest";
import { isPlainTextInputFocused } from "./useGlobalShortcuts";

describe("isPlainTextInputFocused", () => {
  it("is true for a real <input>, e.g. TabRenameInput", () => {
    const input = document.createElement("input");
    expect(isPlainTextInputFocused(input)).toBe(true);
  });

  it("is true for a <textarea>", () => {
    const textarea = document.createElement("textarea");
    expect(isPlainTextInputFocused(textarea)).toBe(true);
  });

  it("is false for null (nothing focused)", () => {
    expect(isPlainTextInputFocused(null)).toBe(false);
  });

  it("is false for a plain div", () => {
    const div = document.createElement("div");
    expect(isPlainTextInputFocused(div)).toBe(false);
  });

  // CodeMirror's editor surface is contenteditable, not <textarea> — a
  // blanket contenteditable exclusion would regress ⌘F (search.focus)
  // while the cursor is in the code editor, so this must stay false here.
  it("is false for a contenteditable div (e.g. the CodeMirror editor surface)", () => {
    const div = document.createElement("div");
    div.contentEditable = "true";
    expect(isPlainTextInputFocused(div)).toBe(false);
  });

  it("is false for a button", () => {
    const button = document.createElement("button");
    expect(isPlainTextInputFocused(button)).toBe(false);
  });
});
