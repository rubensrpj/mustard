// Heavy half of the docked CodeViewerPanel — imports the syntax highlighter
// (via `CodeBlock`) and the Markdown renderer. Kept in its own module so the
// panel's `index.tsx` can pull it in with `React.lazy`, keeping the highlighter
// out of the main bundle (it only loads once a file is actually opened).
//
// Renders the ACTIVE tab's content. Markdown files (`md`/`markdown`/`mdx`)
// render with the rich Markdown component; everything else (json, ts, cs, rs,
// py, yaml, …) goes through CodeBlock, which maps extension → Prism grammar and
// falls back to plain text for unknown ids — so EVERY readable file renders.

import { useState } from "react";
import {
  Copy,
  Check,
  FileCode2,
  AlertCircle,
  FileX2,
  ScrollText,
  Loader2,
} from "lucide-react";
import { CodeBlock } from "@/components/page/CodeBlock";
import { Markdown } from "@/components/page/Markdown";
import { useFileContent } from "@/hooks/useFileContent";
import { cn } from "@/lib/utils";
import type { OpenTab } from "@/lib/code-viewer-store";

/** Is this a markdown file? Render with the rich Markdown component instead of
 *  the code highlighter. The backend emits `language` as the lowercase
 *  extension, so a `.md` file arrives as `md` and a `.markdown` as `markdown` —
 *  accept both (plus `mdx`). */
function isMarkdown(language: string): boolean {
  const l = language.toLowerCase();
  return l === "md" || l === "markdown" || l === "mdx";
}

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  const onCopy = () => {
    try {
      void navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    } catch {
      // Clipboard can fail on locked-down builds — the user can still select
      // and copy manually, so we never surface an error.
    }
  };
  return (
    <button
      type="button"
      onClick={onCopy}
      aria-label="Copiar conteúdo"
      className={cn(
        "shrink-0 inline-flex items-center gap-1 rounded-md px-2 py-1",
        "text-[11px] text-muted-foreground hover:text-foreground",
        "hover:bg-muted/40 transition-colors",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[--primary]",
      )}
    >
      {copied ? <Check className="h-3.5 w-3.5" aria-hidden /> : <Copy className="h-3.5 w-3.5" aria-hidden />}
      {copied ? "Copiado" : "Copiar"}
    </button>
  );
}

/** Centered notice for the non-readable / loading states. */
function Notice({ icon: Icon, text, spin }: { icon: typeof AlertCircle; text: string; spin?: boolean }) {
  return (
    <div className="flex flex-col items-center justify-center gap-2 py-12 text-muted-foreground">
      <Icon className={cn("h-6 w-6 text-muted-foreground/70", spin && "animate-spin")} aria-hidden />
      <p className="text-[13px]">{text}</p>
    </div>
  );
}

/**
 * Body for ONE active tab — fetches its content via `useFileContent` (TanStack
 * Query, cached per file) and renders the header + the type-appropriate body.
 * Keyed by tab `id` upstream so switching tabs remounts cleanly.
 */
export default function CodeViewerBody({ tab }: { tab: OpenTab }) {
  const { data, isLoading } = useFileContent(tab.repoPath, tab.relPath);
  const content = data?.content ?? "";
  const language = data?.language ?? "";
  const md = isMarkdown(language);

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* Header: file name (mono) + language label + copy */}
      <div className="flex items-center gap-2 px-4 py-2.5 border-b border-border/60">
        <FileCode2 className="h-4 w-4 shrink-0 text-muted-foreground" aria-hidden />
        <span className="font-mono text-[13px] truncate" title={tab.fileName}>
          {tab.fileName}
        </span>
        {language && (
          <span className="shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wide bg-muted/60 text-muted-foreground">
            {language}
          </span>
        )}
        <span className="flex-1" />
        {data?.readable && !data?.is_binary && content && <CopyButton text={content} />}
      </div>

      {/* Body */}
      <div className="overflow-auto min-h-0 flex-1">
        {isLoading && !data ? (
          <Notice icon={Loader2} text="Carregando…" spin />
        ) : !data?.readable ? (
          <Notice icon={AlertCircle} text="Não foi possível abrir o arquivo." />
        ) : data.is_binary ? (
          <Notice icon={FileX2} text="Arquivo binário (não exibível)." />
        ) : (
          <div className="px-4 py-4 flex flex-col gap-2">
            {data.truncated && (
              <div className="flex items-center gap-2 rounded-md border border-[--primary]/30 bg-[--primary]/5 px-3 py-1.5 text-[11px] text-muted-foreground">
                <ScrollText className="h-3.5 w-3.5 shrink-0 text-[--primary]" aria-hidden />
                Exibindo o início (arquivo grande).
              </div>
            )}
            {md ? (
              <Markdown content={content} />
            ) : (
              <CodeBlock code={content} lang={language} showLineNumbers />
            )}
          </div>
        )}
      </div>
    </div>
  );
}
