// Heavy half of the CodeViewer — imports the syntax highlighter (via
// `CodeBlock`) and the Markdown renderer. Kept in its own module so the parent
// `index.tsx` can pull it in with `React.lazy`, keeping the highlighter out of
// the main bundle (it only loads when the viewer actually opens).

import { useState } from "react";
import { Copy, Check, FileCode2, AlertCircle, FileX2, ScrollText } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";
import { CodeBlock } from "@/components/page/CodeBlock";
import { Markdown } from "@/components/page/Markdown";
import { cn } from "@/lib/utils";
import type { CodeViewerProps } from "./types";

/** Is this a markdown file? Render with the rich Markdown component instead of
 *  the code highlighter (matches the tracer's Read treatment). */
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

/** Centered notice for the non-readable states (binary / unreadable). */
function Notice({ icon: Icon, text }: { icon: typeof AlertCircle; text: string }) {
  return (
    <div className="flex flex-col items-center justify-center gap-2 py-12 text-muted-foreground">
      <Icon className="h-6 w-6 text-muted-foreground/70" aria-hidden />
      <p className="text-[13px]">{text}</p>
    </div>
  );
}

export default function CodeViewerContent({
  open,
  onOpenChange,
  fileName,
  content,
  language,
  isBinary = false,
  truncated = false,
  readable = true,
}: CodeViewerProps) {
  const md = isMarkdown(language);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        // Full-screen takeover — override the small default max-width (mirrors
        // SpecMarkdownViewer's modal sizing).
        className="!max-w-[min(96vw,1100px)] !w-[min(96vw,1100px)] h-[90vh] grid-rows-[auto_1fr] p-0 gap-0 overflow-hidden"
      >
        {/* Header: file name (mono) + language label + copy */}
        <div className="flex items-center gap-2 px-4 pt-4 pb-3 border-b border-border/60">
          <FileCode2 className="h-4 w-4 shrink-0 text-muted-foreground" aria-hidden />
          <DialogTitle className="font-mono text-[13px] truncate" title={fileName}>
            {fileName}
          </DialogTitle>
          {language && (
            <span className="shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wide bg-muted/60 text-muted-foreground">
              {language}
            </span>
          )}
          <span className="flex-1" />
          {readable && !isBinary && content && <CopyButton text={content} />}
        </div>
        <DialogDescription className="sr-only">
          Visualizador de código para {fileName}
        </DialogDescription>

        {/* Body */}
        <div className="overflow-auto min-h-0">
          {!readable ? (
            <Notice icon={AlertCircle} text="Não foi possível abrir o arquivo." />
          ) : isBinary ? (
            <Notice icon={FileX2} text="Arquivo binário (não exibível)." />
          ) : (
            <div className="px-4 py-4 flex flex-col gap-2">
              {truncated && (
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
      </DialogContent>
    </Dialog>
  );
}
