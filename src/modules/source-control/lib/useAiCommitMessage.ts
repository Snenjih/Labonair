import { generateText } from "ai";
import { useState } from "react";
import { buildModel } from "@/modules/ai/lib/agent";
import { EMPTY_PROVIDER_KEYS } from "@/modules/ai/lib/keyring";
import { useChatStore } from "@/modules/ai/store/chatStore";
import { useProvidersStore } from "@/modules/ai/store/providersStore";
import { git } from "./gitInvoke";

const COMMIT_MSG_SYSTEM_PROMPT = `You are a git commit message generator. Given a unified diff, produce a single conventional commit message. Format: type(scope): subject. Subject must be under 72 characters. Types: feat, fix, docs, style, refactor, perf, test, chore, ci. Only output the commit message — no explanation, no markdown, no quotes.`;

export function useAiCommitMessage(repoRoot: string | null, sessionId?: string) {
  const [isGenerating, setIsGenerating] = useState(false);
  const selectedModelId = useChatStore((s) => s.selectedModelId);
  const instances = useProvidersStore((s) => s.instances);
  const instanceKeys = useProvidersStore((s) => s.instanceKeys);

  async function generate(): Promise<string | null> {
    if (!repoRoot) return null;
    setIsGenerating(true);
    try {
      // 1. Get staged diff, fall back to unstaged if nothing staged
      let diff = "";
      try {
        diff = await git.getDiff(repoRoot, ".", true, undefined, sessionId);
      } catch {
        // ignore
      }
      if (!diff.trim()) {
        try {
          diff = await git.getDiff(repoRoot, ".", false, undefined, sessionId);
        } catch {
          // ignore
        }
      }
      if (!diff.trim()) return null;

      // 2. Reuse whatever model the user currently has selected in the AI
      // chat panel — same resolution path the chat agent itself uses, so the
      // commit-message generator never silently picks a different provider.
      if (instances.length === 0) {
        throw new Error("No AI provider configured. Add one in Settings → AI.");
      }
      const model = await buildModel(selectedModelId, EMPTY_PROVIDER_KEYS, {}, instances, instanceKeys);

      const { text } = await generateText({
        model,
        system: COMMIT_MSG_SYSTEM_PROMPT,
        prompt: diff.slice(0, 10_000),
      });

      return text.trim();
    } finally {
      setIsGenerating(false);
    }
  }

  return { generate, isGenerating };
}
