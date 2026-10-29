import { useEffect, useRef, useState } from 'react';
import type { ResponseMedia } from './types';

/** Pages rendered at most; very long documents should be saved and opened elsewhere. */
const PAGE_LIMIT = 50;

function bytes(base64: string) {
  const binary = atob(base64);
  const result = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    result[index] = binary.charCodeAt(index);
  }
  return result;
}

/** Renders a PDF with pdf.js (loaded on first use) into one canvas per page. */
function PdfPreview({ data }: { data: string }) {
  const host = useRef<HTMLDivElement>(null);
  const [state, setState] = useState<{
    data: string;
    status: 'loading' | 'done' | 'error';
    pages: number;
  }>({ data, status: 'loading', pages: 0 });
  if (state.data !== data) {
    setState({ data, status: 'loading', pages: 0 });
  }

  useEffect(() => {
    let cancelled = false;
    let destroy: (() => Promise<void>) | undefined;
    const target = host.current;
    target?.replaceChildren();
    void (async () => {
      const pdfjs = await import('pdfjs-dist');
      if (!pdfjs.GlobalWorkerOptions.workerPort) {
        // Bundled by Vite as a classic .js worker, so the webview needs no .mjs MIME mapping.
        const { default: PdfWorker } = await import('pdfjs-dist/build/pdf.worker.min.mjs?worker');
        pdfjs.GlobalWorkerOptions.workerPort = new PdfWorker();
      }
      const task = pdfjs.getDocument({ data: bytes(data), enableXfa: false });
      destroy = () => task.destroy();
      const document = await task.promise;
      const shown = Math.min(document.numPages, PAGE_LIMIT);
      const width = Math.max(320, (target?.clientWidth ?? 800) - 8);
      const ratio = window.devicePixelRatio || 1;
      for (let number = 1; number <= shown && !cancelled; number += 1) {
        const page = await document.getPage(number);
        const unscaled = page.getViewport({ scale: 1 });
        const viewport = page.getViewport({ scale: (width / unscaled.width) * ratio });
        const canvas = window.document.createElement('canvas');
        canvas.width = Math.floor(viewport.width);
        canvas.height = Math.floor(viewport.height);
        canvas.style.width = `${Math.floor(viewport.width / ratio)}px`;
        canvas.setAttribute('aria-label', `Page ${number} of ${document.numPages}`);
        canvas.setAttribute('role', 'img');
        target?.append(canvas);
        await page.render({ canvas, viewport }).promise;
      }
      if (!cancelled) {
        setState({ data, status: 'done', pages: document.numPages });
      }
    })().catch(() => {
      if (!cancelled) {
        setState({ data, status: 'error', pages: 0 });
      }
    });
    return () => {
      cancelled = true;
      void destroy?.();
    };
  }, [data]);

  return (
    <div className="media-preview media-preview-pdf">
      {state.status === 'loading' && <p role="status">Rendering PDF…</p>}
      {state.status === 'error' && (
        <p role="alert">
          This PDF could not be rendered. Use “Send & save response…” to open it in another
          application.
        </p>
      )}
      {state.status === 'done' && state.pages > PAGE_LIMIT && (
        <p role="status">
          Showing the first {PAGE_LIMIT} of {state.pages} pages.
        </p>
      )}
      <div ref={host} className="media-pages" />
    </div>
  );
}

export function MediaPreview({ media }: { media: ResponseMedia }) {
  if (media.data === null) {
    return (
      <p>
        The response is too large to preview (limit 20 MiB). Use “Send & save response…” to save it
        to a file.
      </p>
    );
  }
  if (media.mime === 'application/pdf') {
    return <PdfPreview data={media.data} />;
  }
  return (
    <div className="media-preview">
      <img src={`data:${media.mime};base64,${media.data}`} alt="Response image" />
    </div>
  );
}
