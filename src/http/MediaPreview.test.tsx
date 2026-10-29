import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, expect, it, vi } from 'vitest';
import { ResponseView } from './ResponseView';
import type { ExecutionResult } from './types';

const pdf = vi.hoisted(() => ({
  getDocument: vi.fn(),
  rendered: [] as number[],
  options: { workerPort: null as unknown },
}));
vi.mock('pdfjs-dist', () => ({
  GlobalWorkerOptions: pdf.options,
  getDocument: pdf.getDocument,
}));
vi.mock('pdfjs-dist/build/pdf.worker.min.mjs?worker', () => ({
  default: class {
    synthetic = true;
  },
}));

const binary: ExecutionResult = {
  outcome: 'response',
  status: 200,
  elapsedMs: 3,
  byteCount: 10,
  text: null,
  jsonText: null,
  headers: [],
  contentKind: 'binary',
  truncated: false,
  saved: false,
  message: null,
};

beforeEach(() => {
  pdf.rendered = [];
  pdf.getDocument.mockImplementation(() => ({
    destroy: vi.fn(() => Promise.resolve()),
    promise: Promise.resolve({
      numPages: 2,
      getPage: (number: number) =>
        Promise.resolve({
          getViewport: ({ scale }: { scale: number }) => ({
            width: 600 * scale,
            height: 800 * scale,
          }),
          render: () => {
            pdf.rendered.push(number);
            return { promise: Promise.resolve() };
          },
        }),
    }),
  }));
});

it('opens images in the Preview tab and keeps Raw and Headers available', async () => {
  const user = userEvent.setup();
  render(
    <ResponseView
      result={{ ...binary, media: { mime: 'image/png', data: 'iVBORw0KGgo=' } }}
      running={false}
    />,
  );
  expect(screen.getByRole('tab', { name: 'Preview' })).toHaveAttribute('aria-selected', 'true');
  expect(screen.getByRole('img', { name: 'Response image' })).toHaveAttribute(
    'src',
    'data:image/png;base64,iVBORw0KGgo=',
  );
  await user.click(screen.getByRole('tab', { name: 'Raw' }));
  expect(screen.getByRole('tabpanel')).toHaveTextContent(
    'Binary image/png content. The Preview tab shows it.',
  );
  expect(screen.getByRole('tab', { name: 'JSON' })).toBeDisabled();
});

it('renders every PDF page with pdf.js from the decoded response bytes', async () => {
  render(
    <ResponseView
      result={{ ...binary, media: { mime: 'application/pdf', data: btoa('%PDF-1.7') } }}
      running={false}
    />,
  );
  const panel = screen.getByRole('tabpanel');
  expect(within(panel).getByRole('status')).toHaveTextContent('Rendering PDF…');
  await waitFor(() => expect(within(panel).queryByRole('status')).not.toBeInTheDocument());
  expect(within(panel).getByRole('img', { name: 'Page 1 of 2' })).toBeInTheDocument();
  expect(within(panel).getByRole('img', { name: 'Page 2 of 2' })).toBeInTheDocument();
  expect(pdf.rendered).toEqual([1, 2]);
  const [{ data, enableXfa }] = pdf.getDocument.mock.calls[0] as [
    { data: Uint8Array; enableXfa: boolean },
  ];
  expect(new TextDecoder().decode(data)).toBe('%PDF-1.7');
  expect(enableXfa).toBe(false);
  expect(pdf.options.workerPort).toMatchObject({ synthetic: true });
});

it('reports PDFs that cannot be rendered and media above the preview limit', async () => {
  pdf.getDocument.mockImplementation(() => ({
    destroy: () => Promise.resolve(),
    promise: Promise.reject(new Error('synthetic failure')),
  }));
  const { rerender } = render(
    <ResponseView
      result={{ ...binary, media: { mime: 'application/pdf', data: btoa('broken') } }}
      running={false}
    />,
  );
  expect(await screen.findByRole('alert')).toHaveTextContent('This PDF could not be rendered.');
  rerender(
    <ResponseView
      result={{ ...binary, media: { mime: 'application/pdf', data: null } }}
      running={false}
    />,
  );
  expect(screen.getByRole('tabpanel')).toHaveTextContent(
    'The response is too large to preview (limit 20 MiB).',
  );
});
