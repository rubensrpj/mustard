import { useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Copy, Check } from "lucide-react";
import type { ComponentProps } from "react";

type CodeProps = ComponentProps<"code"> & { className?: string };

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  const handleCopy = () => {
    navigator.clipboard.writeText(text).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  };
  return (
    <button
      onClick={handleCopy}
      className="absolute top-2 right-2 opacity-0 group-hover:opacity-100 transition-opacity p-1 rounded bg-muted hover:bg-muted/80 text-muted-foreground"
      aria-label="Copy code"
    >
      {copied ? <Check size={12} /> : <Copy size={12} />}
    </button>
  );
}

export function Markdown({ content }: { content: string }) {
  return (
    <div className="max-w-[720px] leading-relaxed text-foreground">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          pre: ({ children, ...props }) => {
            // Extract raw text from nested code element for copy
            const codeEl = (children as React.ReactElement<{ children?: unknown }>)?.props;
            const rawText = typeof codeEl?.children === "string" ? codeEl.children : "";
            return (
              <div className="group relative rounded-md border border-border bg-muted/30 my-3 overflow-hidden">
                <pre className="px-3 py-2 text-xs font-mono overflow-x-auto" {...props}>
                  {children}
                </pre>
                <CopyButton text={rawText} />
              </div>
            );
          },
          code: ({ className, children, ...props }: CodeProps) => {
            const text = typeof children === "string" ? children : "";
            const isBlock = (className?.startsWith("language-") ?? false) || text.includes("\n");
            if (isBlock) {
              return (
                <code className={`font-mono text-xs ${className ?? ""}`} {...props}>
                  {children}
                </code>
              );
            }
            return (
              <code
                className="font-mono text-[0.85em] px-1 py-0.5 rounded bg-muted/60 text-foreground"
                {...props}
              >
                {children}
              </code>
            );
          },
          h1: (props) => <h1 className="text-2xl font-semibold tracking-tight mt-6 mb-3" {...props} />,
          h2: (props) => <h2 className="text-xl font-semibold mt-5 mb-2 pb-1 border-b border-border/60" {...props} />,
          h3: (props) => <h3 className="text-lg font-medium mt-4 mb-2" {...props} />,
          h4: (props) => <h4 className="text-base font-medium mt-3 mb-1" {...props} />,
          p: (props) => <p className="text-sm leading-relaxed text-foreground/90 my-2" {...props} />,
          ul: (props) => <ul className="my-2 pl-5 space-y-1 text-sm list-disc marker:text-muted-foreground/60" {...props} />,
          ol: (props) => <ol className="my-2 pl-5 space-y-1 text-sm list-decimal marker:text-muted-foreground/60" {...props} />,
          li: (props) => <li className="leading-relaxed" {...props} />,
          a: (props) => <a className="text-primary underline underline-offset-4 decoration-primary/40 hover:decoration-primary" {...props} />,
          strong: (props) => <strong className="font-semibold text-foreground" {...props} />,
          blockquote: (props) => (
            <blockquote className="border-l-2 border-border pl-3 italic text-muted-foreground my-3" {...props} />
          ),
          hr: () => <hr className="my-6 border-border/60" />,
          table: (props) => (
            <div className="overflow-x-auto my-3">
              <table className="text-sm border-collapse w-full" {...props} />
            </div>
          ),
          th: (props) => <th className="text-left font-medium px-2 py-1 border-b border-border" {...props} />,
          td: (props) => <td className="px-2 py-1 border-b border-border/60" {...props} />,
          input: (props) =>
            props.type === "checkbox" ? (
              <input
                type="checkbox"
                disabled
                checked={Boolean(props.checked)}
                readOnly
                className="translate-y-[1px] mr-1"
              />
            ) : (
              <input {...props} />
            ),
        }}
      >
        {content}
      </ReactMarkdown>
    </div>
  );
}
