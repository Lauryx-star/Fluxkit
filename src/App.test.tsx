import { selectOption } from './test/selectOption';
import { render, screen, within, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import App from './App';
import { getAppInfo, isDesktop } from './native';

vi.mock('./native', () => ({
  isDesktop: vi.fn(),
  getAppInfo: vi.fn(),
  restoreLastProject: vi.fn(async () => null),
  rememberProject: vi.fn(async () => {}),
}));
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({ onCloseRequested: async () => () => {}, destroy: async () => {} }),
}));

beforeEach(() => {
  vi.mocked(isDesktop).mockReturnValue(false);
});

describe('desktop foundation', () => {
  it('starts empty without calling native APIs in browser preview', async () => {
    render(<App />);
    expect(screen.getByRole('heading', { name: /less friction.*more flow/i })).toBeVisible();
    expect(await screen.findByText('Browser preview')).toBeVisible();
    expect(getAppInfo).not.toHaveBeenCalled();
  });

  it('keeps edited drafts separate when switching and closing tabs', async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(within(screen.getByRole('main')).getByRole('button', { name: 'New request' }));
    await user.type(
      screen.getByRole('textbox', { name: 'Request URL' }),
      'https://example.test/one',
    );
    await user.click(screen.getByRole('button', { name: 'New request tab' }));
    await selectOption(user, screen.getByRole('combobox', { name: 'HTTP method' }), 'POST');
    await user.type(
      screen.getByRole('textbox', { name: 'Request URL' }),
      'https://example.test/two',
    );
    const tabs = screen.getByLabelText('Request tabs');
    await user.click(within(tabs).getByRole('button', { name: 'GET Request 1' }));
    expect(screen.getByRole('textbox', { name: 'Request URL' })).toHaveValue(
      'https://example.test/one',
    );
    await user.click(screen.getByRole('button', { name: 'Close Request 1' }));
    await user.click(screen.getByRole('button', { name: 'Continue' }));
    expect(screen.getByRole('textbox', { name: 'Request URL' })).toHaveValue(
      'https://example.test/two',
    );
    expect(screen.getByRole('combobox', { name: 'HTTP method' })).toHaveValue('POST');
    expect(screen.getByRole('button', { name: 'Send' })).toBeDisabled();
    await user.click(screen.getByRole('button', { name: 'Close Request 2' }));
    await user.click(screen.getByRole('button', { name: 'Continue' }));
    expect(screen.queryByRole('textbox', { name: 'Request URL' })).not.toBeInTheDocument();
  });

  it('undoes request body edits per tab with Ctrl+Z and redoes them with Ctrl+Shift+Z', async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(screen.getByRole('button', { name: 'New request tab' }));
    await user.click(screen.getByRole('tab', { name: 'Body' }));
    await selectOption(user, screen.getByRole('combobox', { name: 'Body type' }), 'text');
    await user.type(screen.getByRole('textbox', { name: 'Request body' }), 'first tab');
    await user.click(screen.getByRole('button', { name: 'New request tab' }));
    await user.click(screen.getByRole('tab', { name: 'Body' }));
    await selectOption(user, screen.getByRole('combobox', { name: 'Body type' }), 'text');
    const second = screen.getByRole('textbox', { name: 'Request body' });
    await user.type(second, 'second tab');
    await user.keyboard('{Control>}z{/Control}');
    expect(second).toHaveValue('');
    await user.keyboard('{Control>}{Shift>}z{/Shift}{/Control}');
    expect(second).toHaveValue('second tab');
    const tabs = screen.getByLabelText('Request tabs');
    await user.click(within(tabs).getByRole('button', { name: 'GET Request 1' }));
    await user.click(screen.getByRole('tab', { name: 'Body' }));
    const first = screen.getByRole('textbox', { name: 'Request body' });
    expect(first).toHaveValue('first tab');
    await user.click(first);
    await user.keyboard('{Control>}z{/Control}');
    expect(first).toHaveValue('');
    await user.keyboard('{Control>}z{/Control}');
    expect(first).toHaveValue('');
  });

  it('undoes URL, parameter and header edits field by field', async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(screen.getByRole('button', { name: 'New request tab' }));
    const url = screen.getByRole('textbox', { name: 'Request URL' });
    await user.type(url, 'https://example.test/items?page=2');
    await user.click(screen.getByRole('tab', { name: 'Params' }));
    const value = screen.getByRole('textbox', { name: 'Parameter value 1' });
    await user.type(value, '0');
    expect(url).toHaveValue('https://example.test/items?page=20');
    await user.keyboard('{Control>}z{/Control}');
    expect(value).toHaveValue('2');
    expect(url).toHaveValue('https://example.test/items?page=2');
    await user.click(url);
    await user.keyboard('{Control>}z{/Control}');
    expect(url).toHaveValue('');
    await user.keyboard('{Control>}y{/Control}');
    expect(url).toHaveValue('https://example.test/items?page=2');
    await user.click(screen.getByRole('tab', { name: 'Headers' }));
    const header = screen.getByRole('textbox', { name: 'Header key 1' });
    await user.type(header, 'Accept');
    await user.keyboard('{Control>}z{/Control}');
    expect(header).toHaveValue('');
  });

  it('mirrors URL query parameters in the parameter table in both directions', async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(screen.getByRole('button', { name: 'New request tab' }));
    const url = screen.getByRole('textbox', { name: 'Request URL' });
    await user.type(url, 'https://example.test/report?format=pdf&year=');
    expect(url).toHaveValue('https://example.test/report?format=pdf&year=');
    await user.click(screen.getByRole('tab', { name: 'Params' }));
    expect(screen.getByRole('textbox', { name: 'Parameter key 1' })).toHaveValue('format');
    expect(screen.getByRole('textbox', { name: 'Parameter value 1' })).toHaveValue('pdf');
    expect(screen.getByRole('textbox', { name: 'Parameter key 2' })).toHaveValue('year');
    await user.type(screen.getByRole('textbox', { name: 'Parameter value 2' }), '2026');
    expect(url).toHaveValue('https://example.test/report?format=pdf&year=2026');
    await user.click(screen.getByRole('checkbox', { name: 'Enable Parameter 1' }));
    expect(url).toHaveValue('https://example.test/report?year=2026');
    await user.clear(url);
    await user.paste('https://example.test/other?page=3');
    expect(screen.getByRole('textbox', { name: 'Parameter key 1' })).toHaveValue('format');
    expect(screen.getByRole('checkbox', { name: 'Enable Parameter 1' })).not.toBeChecked();
    expect(screen.getByRole('textbox', { name: 'Parameter key 2' })).toHaveValue('page');
    expect(screen.getByRole('textbox', { name: 'Parameter value 2' })).toHaveValue('3');
  });

  it('edits header rows and preserves body content across configuration tabs', async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(screen.getByRole('button', { name: 'New request tab' }));
    await user.click(screen.getByRole('tab', { name: 'Headers' }));
    await user.type(screen.getByRole('textbox', { name: 'Header key 1' }), 'Accept');
    await user.type(screen.getByRole('textbox', { name: 'Header value 1' }), 'application/json');
    await user.click(screen.getByRole('checkbox', { name: 'Enable Header 1' }));
    expect(screen.getByRole('checkbox', { name: 'Enable Header 1' })).not.toBeChecked();
    await user.click(screen.getByRole('button', { name: 'Add header' }));
    expect(screen.getByRole('textbox', { name: 'Header key 2' })).toBeVisible();
    await user.click(screen.getByRole('button', { name: 'Remove Header 2' }));
    expect(screen.queryByRole('textbox', { name: 'Header key 2' })).not.toBeInTheDocument();
    await user.click(screen.getByRole('tab', { name: 'Body' }));
    await selectOption(user, screen.getByRole('combobox', { name: 'Body type' }), 'text');
    await user.type(screen.getByRole('textbox', { name: 'Request body' }), 'hello');
    await user.click(screen.getByRole('tab', { name: 'Params' }));
    await user.click(screen.getByRole('tab', { name: 'Body' }));
    expect(screen.getByRole('textbox', { name: 'Request body' })).toHaveValue('hello');
  });

  it('supports keyboard navigation between request configuration tabs', async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(screen.getByRole('button', { name: 'New request tab' }));
    await user.click(screen.getByRole('tab', { name: 'Params' }));
    await user.keyboard('{ArrowRight}');
    expect(screen.getByRole('tab', { name: 'Headers' })).toHaveFocus();
    expect(screen.getByRole('tabpanel', { name: 'Headers' })).toBeVisible();
    await user.keyboard('{End}');
    expect(screen.getByRole('tab', { name: 'Auth' })).toHaveFocus();
    await user.keyboard('{Home}');
    expect(screen.getByRole('tab', { name: 'Params' })).toHaveFocus();
  });

  it('reports a failed native connection without pretending to be connected', async () => {
    vi.mocked(isDesktop).mockReturnValue(true);
    vi.mocked(getAppInfo).mockRejectedValue(new Error('Unavailable'));
    render(<App />);
    expect(await screen.findByText('Desktop connection unavailable')).toBeVisible();
  });

  it('shows native application metadata after connecting', async () => {
    vi.mocked(isDesktop).mockReturnValue(true);
    vi.mocked(getAppInfo).mockResolvedValue({ name: 'Fluxkit', version: '0.1.0' });
    render(<App />);
    await waitFor(() => expect(screen.getByText('Desktop connected · v0.1.0')).toBeVisible());
  });
});
