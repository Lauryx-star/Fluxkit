import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, it } from 'vitest';
import { ResponseView } from './ResponseView';
import type { ExecutionResult } from './types';
const response: ExecutionResult = {
  outcome: 'response',
  status: 200,
  elapsedMs: 3,
  byteCount: 28,
  text: '<script>window.evil()</script>',
  jsonText: null,
  headers: [{ name: 'x-example', value: '<img src=x onerror=evil()>' }],
  contentKind: 'text',
  truncated: false,
  saved: false,
  message: null,
};
it('keeps the HTTP result visible when history recording fails', () => {
  render(
    <ResponseView
      result={{ ...response, historyWarning: 'History could not be recorded.' }}
      running={false}
    />,
  );
  expect(screen.getByText('History could not be recorded.')).toBeVisible();
  expect(screen.getByText(/HTTP 200/)).toBeVisible();
  expect(screen.getByRole('tabpanel')).toHaveTextContent(response.text!);
});
it('shows unknown token expiry and known expiry as dispatch metadata without token values', () => {
  const { rerender } = render(
    <ResponseView
      result={{ ...response, tokenStatus: { state: 'unknownExpiry', expiresAt: null } }}
      running={false}
    />,
  );
  expect(screen.getByText('Authentication token at dispatch: Expiry unknown.')).toBeVisible();
  rerender(
    <ResponseView
      result={{ ...response, tokenStatus: { state: 'valid', expiresAt: 1792756800 } }}
      running={false}
    />,
  );
  expect(
    screen.getByText(/Authentication token at dispatch: Token available · expires/),
  ).toBeVisible();
});
it('renders untrusted body and headers as inert text and supports keyboard tab navigation', async () => {
  const user = userEvent.setup();
  const { container } = render(<ResponseView result={response} running={false} />);
  expect(screen.getByRole('tabpanel')).toHaveTextContent('<script>window.evil()</script>');
  expect(container.querySelector('script')).toBeNull();
  screen.getByRole('tab', { name: 'Raw' }).focus();
  await user.keyboard('{ArrowRight}');
  expect(screen.getByRole('tab', { name: 'Headers' })).toHaveFocus();
  expect(screen.getByRole('table')).toHaveTextContent('<img src=x onerror=evil()>');
  expect(container.querySelector('img')).toBeNull();
});
it('shows timeout without inventing an HTTP status and labels truncated and binary responses', () => {
  const { rerender } = render(
    <ResponseView
      result={{
        ...response,
        outcome: 'timeout',
        status: null,
        message: 'Request timed out.',
        text: null,
      }}
      running={false}
    />,
  );
  expect(screen.getByRole('alert')).toHaveTextContent('Request timed out.');
  expect(screen.queryByText(/HTTP 200/)).not.toBeInTheDocument();
  rerender(
    <ResponseView
      result={{ ...response, text: null, contentKind: 'binary', truncated: true }}
      running={false}
    />,
  );
  expect(screen.getByText(/Preview is limited to 1 MiB/)).toBeVisible();
  expect(screen.getByRole('tabpanel')).toHaveTextContent(
    'Binary, compressed, or non-UTF-8 content',
  );
});
