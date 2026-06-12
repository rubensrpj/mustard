/**
 * Props for the CodeViewer modal. APRESENTACIONAL — it receives the file
 * content already fetched (the next step wires the data source: Git card /
 * most-touched / README / tracer). Shape mirrors the backend `FileContent`
 * projection so a caller can spread a `useFileContent` result straight in.
 */
export interface CodeViewerProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** File path / name shown in the header (mono). */
  fileName: string;
  /** The file's text. Empty for binary / unreadable files. */
  content: string;
  /** Language hint (extension / alias / Prism id). `md`/`markdown` renders
   *  with the rich Markdown component; everything else with CodeBlock. */
  language: string;
  /** `true` → "arquivo binário (não exibível)" state. */
  isBinary?: boolean;
  /** `true` → a "exibindo o início (arquivo grande)" note above the content. */
  truncated?: boolean;
  /** `false` → "não foi possível abrir o arquivo" state. Defaults to `true`. */
  readable?: boolean;
}
