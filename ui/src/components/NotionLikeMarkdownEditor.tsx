import { useCallback, useEffect, useLayoutEffect, useRef } from "react";
import { en } from "@blocknote/core/locales";
import { useCreateBlockNote } from "@blocknote/react";
import { BlockNoteView } from "@blocknote/ariakit";
import "@blocknote/core/fonts/inter.css";
import "@blocknote/ariakit/style.css";

type Props = {
  markdown: string;
  onMarkdownChange: (markdown: string) => void;
  placeholder?: string;
  "aria-label"?: string;
  /** Shown in print / export filenames */
  documentTitle?: string;
  className?: string;
};

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function safeFilename(s: string): string {
  const t = s.trim().replace(/[/\\?%*:|"<>]/g, "-").slice(0, 80);
  return t || "summary";
}

export function NotionLikeMarkdownEditor({
  markdown,
  onMarkdownChange,
  placeholder = "Write something… Type / for blocks.",
  "aria-label": ariaLabel,
  documentTitle = "Summary",
  className,
}: Props) {
  const lastInternalMdRef = useRef<string | null>(null);
  const onMarkdownChangeRef = useRef(onMarkdownChange);
  onMarkdownChangeRef.current = onMarkdownChange;

  const editor = useCreateBlockNote(
    {
      dictionary: en,
      placeholders: {
        default: placeholder,
        emptyDocument: placeholder,
      },
    },
    []
  );

  const pushMarkdown = useCallback(() => {
    const md = editor.blocksToMarkdownLossy();
    if (md === lastInternalMdRef.current) {
      return;
    }
    lastInternalMdRef.current = md;
    onMarkdownChangeRef.current(md);
  }, [editor]);

  useLayoutEffect(() => {
    if (markdown === lastInternalMdRef.current) {
      return;
    }
    const blocks = editor.tryParseMarkdownToBlocks(markdown || "");
    const toInsert = blocks.length > 0 ? blocks : [{ type: "paragraph" as const }];
    editor.replaceBlocks(
      editor.document.map((b) => b.id),
      toInsert
    );
    lastInternalMdRef.current = editor.blocksToMarkdownLossy();
  }, [markdown, editor]);

  useEffect(() => {
    return editor.onChange(() => {
      pushMarkdown();
    });
  }, [editor, pushMarkdown]);

  const handlePrintOrPdf = useCallback(() => {
    const title = documentTitle.trim() || "Summary";
    const bodyHtml = editor.blocksToFullHTML();
    const styles = `
      body { font-family: Inter, ui-sans-serif, system-ui, sans-serif; color: #0f1218; max-width: 720px; margin: 40px auto;
        padding: 0 24px 48px; line-height: 1.6; font-size: 15px; }
      .bn-block-outer { margin: 0.35em 0; }
      h1 { font-size: 1.75rem; font-weight: 700; letter-spacing: -0.02em; margin: 1em 0 0.35em; }
      h2 { font-size: 1.35rem; font-weight: 650; margin: 0.9em 0 0.3em; }
      h3 { font-size: 1.15rem; font-weight: 600; margin: 0.85em 0 0.25em; }
      ul, ol { padding-left: 1.35rem; }
      pre, code { font-family: ui-monospace, monospace; font-size: 0.9em; }
      pre { background: #f4f4f5; padding: 12px 14px; border-radius: 8px; overflow: auto; }
      code { background: #f4f4f5; padding: 0.12em 0.35em; border-radius: 4px; }
      blockquote { border-left: 3px solid #3dd6c5; margin: 0.75em 0; padding-left: 1em; color: #374151; }
      table { border-collapse: collapse; width: 100%; margin: 0.75em 0; }
      th, td { border: 1px solid #e5e7eb; padding: 8px 10px; text-align: left; }
      @media print { body { margin: 0; padding: 16px; } }
    `;
    const w = window.open("", "_blank");
    if (!w) {
      return;
    }
    w.document.write(
      `<!DOCTYPE html><html><head><meta charset="utf-8"/><title>${escapeHtml(
        title
      )}</title><style>${styles}</style></head><body><article>${bodyHtml}</article></body></html>`
    );
    w.document.close();
    w.focus();
    requestAnimationFrame(() => {
      w.print();
    });
  }, [documentTitle, editor]);

  const handleCopyMarkdown = useCallback(async () => {
    const md = editor.blocksToMarkdownLossy();
    try {
      await navigator.clipboard.writeText(md);
    } catch {
      /* ignore */
    }
  }, [editor]);

  const handleDownloadMarkdown = useCallback(() => {
    const md = editor.blocksToMarkdownLossy();
    const name = safeFilename(documentTitle);
    const blob = new Blob([md], { type: "text/markdown;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `${name}.md`;
    a.click();
    URL.revokeObjectURL(url);
  }, [documentTitle, editor]);

  return (
    <div className={className ? `notion-editor-wrap ${className}` : "notion-editor-wrap"}>
      <div className="notion-editor-actions" role="toolbar" aria-label="Export and print">
        <button type="button" className="btn-ghost btn-small" onClick={handlePrintOrPdf}>
          Print / Save as PDF…
        </button>
        <button type="button" className="btn-ghost btn-small" onClick={() => void handleCopyMarkdown()}>
          Copy Markdown
        </button>
        <button type="button" className="btn-ghost btn-small" onClick={handleDownloadMarkdown}>
          Download .md
        </button>
      </div>
      <div className="notion-editor-surface" aria-label={ariaLabel}>
        <BlockNoteView editor={editor} theme="dark" />
      </div>
    </div>
  );
}
