import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, expect, it, vi } from 'vitest';
import { HistoryPanel } from './HistoryPanel';
import type { HistoryView } from './types';

const bridge = vi.hoisted(() => ({ historyAction: vi.fn() }));
vi.mock('../native', () => bridge);
const empty: HistoryView = { recording: false, entries: [], warning: null };
const recorded: HistoryView = {
  recording: true,
  entries: [
    {
      method: 'POST',
      url: 'https://api.example.test/[redacted]?[redacted]={{token}}',
      status: null,
      elapsedMs: null,
    },
    { method: 'GET', url: '{{baseUrl}}/[redacted]', status: 201, elapsedMs: 9 },
  ],
  warning: null,
};
beforeEach(() => {
  vi.resetAllMocks();
  bridge.historyAction.mockResolvedValue(empty);
});
const close = vi.fn();
function panel(handle = 'project-a', refresh = 0, ask = vi.fn().mockResolvedValue(true)) {
  return (
    <HistoryPanel
      key={handle}
      handle={handle}
      name={handle}
      refresh={refresh}
      ask={ask}
      onClose={close}
    />
  );
}

it('starts opted out, changes recording explicitly, and shows four inert metadata columns', async () => {
  const user = userEvent.setup();
  render(panel());
  const toggle = await screen.findByRole('checkbox', {
    name: 'Record request history for this project',
  });
  await waitFor(() => expect(toggle).toBeEnabled());
  expect(toggle).not.toBeChecked();
  expect(screen.getByText('No recorded requests.')).toBeVisible();
  bridge.historyAction.mockResolvedValueOnce(recorded);
  await user.click(toggle);
  expect(await screen.findByRole('table')).toHaveTextContent('201');
  const table = screen.getByRole('table');
  expect(
    within(table)
      .getAllByRole('columnheader')
      .map((item) => item.textContent),
  ).toEqual(['Method', 'URL', 'Status', 'Time']);
  expect(within(table).getByLabelText('No HTTP status')).toHaveTextContent('—');
  expect(within(table).getByLabelText('No response time')).toHaveTextContent('—');
  expect(table).toHaveTextContent('9 ms');
  expect(table.querySelector('a')).toBeNull();
  expect(table.querySelector('button')).toBeNull();
  expect(bridge.historyAction).toHaveBeenCalledWith('project-a', {
    kind: 'setRecording',
    enabled: true,
  });
});

it('keeps retained entries when disabled and confirms clearing only the selected project', async () => {
  const user = userEvent.setup();
  bridge.historyAction.mockResolvedValueOnce(recorded);
  const ask = vi.fn().mockResolvedValueOnce(false).mockResolvedValueOnce(true);
  render(panel('project-a', 0, ask));
  await screen.findByRole('table');
  bridge.historyAction.mockResolvedValueOnce({ ...recorded, recording: false });
  await user.click(screen.getByRole('checkbox'));
  await waitFor(() => expect(screen.getByRole('checkbox')).not.toBeChecked());
  expect(screen.getByRole('table')).toBeVisible();
  await user.click(screen.getByRole('button', { name: 'Clear history' }));
  expect(bridge.historyAction).toHaveBeenCalledTimes(2);
  bridge.historyAction.mockResolvedValueOnce(empty);
  await user.click(screen.getByRole('button', { name: 'Clear history' }));
  await waitFor(() => expect(screen.queryByRole('table')).not.toBeInTheDocument());
  expect(bridge.historyAction).toHaveBeenLastCalledWith('project-a', { kind: 'clear' });
});

it('discards late loads across projects and refreshes when a send completes', async () => {
  let complete!: (value: HistoryView) => void;
  bridge.historyAction.mockReturnValueOnce(
    new Promise((resolve) => {
      complete = resolve;
    }),
  );
  const { rerender } = render(panel('old-project'));
  await waitFor(() => expect(bridge.historyAction).toHaveBeenCalledTimes(1));
  rerender(panel('new-project'));
  await screen.findByText('No recorded requests.');
  await act(async () => complete(recorded));
  expect(screen.queryByRole('table')).not.toBeInTheDocument();
  bridge.historyAction.mockResolvedValueOnce(recorded);
  rerender(panel('new-project', 1));
  expect(await screen.findByRole('table')).toBeVisible();
  expect(bridge.historyAction).toHaveBeenLastCalledWith('new-project', { kind: 'load' });
});

it('refreshes expiry while open and stops polling when closed', async () => {
  vi.useFakeTimers();
  try {
    const { unmount } = render(panel());
    await act(async () => {
      await Promise.resolve();
    });
    expect(bridge.historyAction).toHaveBeenCalledTimes(1);
    await act(async () => {
      vi.advanceTimersByTime(60_000);
      await Promise.resolve();
    });
    expect(bridge.historyAction).toHaveBeenCalledTimes(2);
    unmount();
    await act(async () => {
      vi.advanceTimersByTime(60_000);
    });
    expect(bridge.historyAction).toHaveBeenCalledTimes(2);
  } finally {
    vi.useRealTimers();
  }
});

it('reports storage errors and permits explicit recovery without executing requests', async () => {
  const user = userEvent.setup();
  bridge.historyAction.mockRejectedValueOnce(
    'Local history is invalid. Clear history explicitly to reset it.',
  );
  render(panel());
  expect(await screen.findByRole('alert')).toHaveTextContent('Clear history explicitly');
  expect(screen.getByRole('checkbox')).toBeDisabled();
  await user.click(screen.getByRole('button', { name: 'Clear history' }));
  expect(await screen.findByText('No recorded requests.')).toBeVisible();
  expect(screen.getByRole('checkbox')).not.toBeChecked();
  fireEvent.keyDown(screen.getByRole('heading', { name: 'Request history' }), { key: 'Escape' });
  expect(close).toHaveBeenCalled();
});

it('focuses an enabled control while loading so Escape can close the dialog', () => {
  bridge.historyAction.mockReturnValue(new Promise(() => {}));
  render(panel());
  const button = screen.getByRole('button', { name: 'Close history' });
  expect(button).toHaveFocus();
  fireEvent.keyDown(button, { key: 'Escape' });
  expect(close).toHaveBeenCalledOnce();
});
