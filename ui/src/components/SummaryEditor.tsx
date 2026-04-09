import { useEffect, useState } from "react";
import { NotionLikeMarkdownEditor } from "./NotionLikeMarkdownEditor";

type Props = {
  initialTitle: string;
  initialBody: string;
  onSave: (title: string, body: string, autosave: boolean) => Promise<void>;
  saving: boolean;
};

export function SummaryEditor({ initialTitle, initialBody, onSave, saving }: Props) {
  const [title, setTitle] = useState(initialTitle);
  const [body, setBody] = useState(initialBody);
  const [dirty, setDirty] = useState(false);

  useEffect(() => {
    setTitle(initialTitle);
    setBody(initialBody);
    setDirty(false);
  }, [initialTitle, initialBody]);

  useEffect(() => {
    if (!dirty) {
      return;
    }
    const t = setTimeout(() => {
      void onSave(title, body, true);
      setDirty(false);
    }, 1200);
    return () => clearTimeout(t);
  }, [dirty, title, body, onSave]);

  return (
    <section className="panel">
      <div className="row">
        <h2 className="section-title">Session summary</h2>
        <button type="button" disabled={saving} onClick={() => void onSave(title, body, false)}>
          {saving ? "Saving…" : "Save revision"}
        </button>
      </div>
      <input
        className="input-title"
        value={title}
        onChange={(e) => {
          setTitle(e.target.value);
          setDirty(true);
        }}
        placeholder="Title"
        aria-label="Summary title"
      />
      <NotionLikeMarkdownEditor
        className="editor-body-notion"
        markdown={body}
        onMarkdownChange={(md) => {
          setBody(md);
          setDirty(true);
        }}
        placeholder="Summary body… Type / for headings, lists, and more."
        aria-label="Summary body"
        documentTitle={title || "Session summary"}
      />
    </section>
  );
}
