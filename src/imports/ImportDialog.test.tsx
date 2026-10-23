import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { ImportDialog } from './ImportDialog';
import type { ImportPreview } from './types';
const preview: ImportPreview = {
  id: 'preview',
  source: 'OpenAPI',
  environmentName: null,
  collectionName: 'Pets',
  version: '3.1.2',
  requestCount: 1,
  folderCount: 1,
  variables: [
    { name: 'baseUrl', hasDefault: true },
    { name: 'token', hasDefault: false },
  ],
  requests: [{ name: '<script>inert()</script>', method: 'GET', url: '{{baseUrl}}/pets' }],
  warnings: ['Only the first server is selected.'],
  blockers: [],
};
describe('OpenAPI preview', () => {
  it('shows mappings and missing values, then explicitly imports with the selected OS-value policy', async () => {
    const user = userEvent.setup();
    const onImport = vi.fn();
    const onCancel = vi.fn();
    render(
      <ImportDialog
        preview={preview}
        projectName="Project"
        busy={false}
        started={false}
        error={null}
        onImport={onImport}
        onCancel={onCancel}
      />,
    );
    expect(screen.getByRole('region', { name: 'Compatibility report' })).toHaveTextContent(
      'first server',
    );
    expect(screen.getByRole('region', { name: 'Imported variables' })).toHaveTextContent(
      '1 variable needs a local value',
    );
    expect(screen.getByText('<script>inert()</script>')).toBeVisible();
    expect(document.querySelector('script')).toBeNull();
    await user.click(screen.getByRole('checkbox'));
    await user.click(screen.getByRole('button', { name: 'Import collection' }));
    expect(onImport).toHaveBeenCalledWith(false);
    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onCancel).toHaveBeenCalled();
  });
  it('blocks incompatible sources and keeps a failed import available for an explicit retry', async () => {
    const user = userEvent.setup();
    const onImport = vi.fn();
    const props = {
      preview,
      projectName: 'Project',
      busy: false,
      started: true,
      error: 'Saved definitions remain available.',
      onImport,
      onCancel: vi.fn(),
    };
    const { rerender } = render(<ImportDialog {...props} />);
    expect(screen.getByRole('alert')).toHaveTextContent('definitions remain');
    expect(screen.getByRole('checkbox')).toBeDisabled();
    await user.click(screen.getByRole('button', { name: 'Retry import' }));
    expect(onImport).toHaveBeenCalledWith(true);
    rerender(
      <ImportDialog
        {...props}
        preview={{ ...preview, blockers: ['Combined security cannot be mapped.'] }}
      />,
    );
    expect(
      within(screen.getByRole('alert', { name: 'Blocking compatibility issues' })).getByText(
        'Combined security cannot be mapped.',
      ),
    ).toBeVisible();
    expect(screen.getByRole('button', { name: 'Retry import' })).toBeDisabled();
  });
});

it('explains a disabled import at the action and takes keyboard focus to the actual blockers', async () => {
  const user = userEvent.setup();
  const onImport = vi.fn();
  render(
    <ImportDialog
      preview={{
        ...preview,
        source: 'Postman',
        blockers: [
          'Nested placeholders in stored variable values must be flattened before import.',
        ],
      }}
      projectName="Project"
      busy={false}
      started={false}
      error={null}
      onImport={onImport}
      onCancel={vi.fn()}
    />,
  );
  const button = screen.getByRole('button', { name: 'Import collection' });
  expect(button).toBeDisabled();
  expect(button).toHaveAccessibleDescription(/Import is blocked by 1 compatibility issue/);
  await user.click(screen.getByRole('button', { name: 'Review import blockers' }));
  expect(screen.getByRole('alert', { name: 'Blocking compatibility issues' })).toHaveFocus();
  expect(onImport).not.toHaveBeenCalled();
});

it('renames a preview variable once, blocks import while editing, and shows updated request URLs', async () => {
  const user = userEvent.setup();
  const onRename = vi.fn(async () => true);
  const props = {
    preview,
    projectName: 'Project',
    busy: false,
    started: false,
    error: null,
    onRename,
    onImport: vi.fn(),
    onCancel: vi.fn(),
  };
  const { rerender } = render(<ImportDialog {...props} />);
  await user.click(screen.getByText('Show variable definitions'));
  await user.click(screen.getByRole('button', { name: 'Rename baseUrl before import' }));
  expect(screen.getByRole('button', { name: 'Import collection' })).toBeDisabled();
  const input = screen.getByRole('textbox', { name: 'Imported variable name' });
  await user.clear(input);
  await user.type(input, 'apiServer');
  await user.click(screen.getByRole('button', { name: 'Apply name' }));
  expect(onRename).toHaveBeenCalledWith('baseUrl', 'apiServer');
  rerender(
    <ImportDialog
      {...props}
      preview={{
        ...preview,
        variables: [
          { sourceName: 'Server URL', name: 'apiServer', hasDefault: true },
          preview.variables[1]!,
        ],
        requests: [{ ...preview.requests[0]!, url: '{{apiServer}}/pets' }],
      }}
    />,
  );
  expect(screen.getByText('{{apiServer}}/pets')).toBeVisible();
  expect(screen.getByRole('button', { name: 'Import collection' })).toBeEnabled();
  rerender(<ImportDialog {...props} started />);
  expect(screen.queryByRole('button', { name: /before import/ })).toBeNull();
});

it('keeps a rejected rename editable and allows cancelling without changing the import', async () => {
  const user = userEvent.setup();
  render(
    <ImportDialog
      preview={preview}
      projectName="Project"
      busy={false}
      started={false}
      error={null}
      onRename={vi.fn(async () => false)}
      onImport={vi.fn()}
      onCancel={vi.fn()}
    />,
  );
  await user.click(screen.getByText('Show variable definitions'));
  await user.click(screen.getByRole('button', { name: 'Rename baseUrl before import' }));
  await user.click(screen.getByRole('button', { name: 'Apply name' }));
  expect(screen.getByRole('textbox', { name: 'Imported variable name' })).toBeVisible();
  expect(screen.getByRole('button', { name: 'Import collection' })).toBeDisabled();
  await user.click(screen.getByRole('button', { name: 'Cancel rename' }));
  expect(screen.getByRole('button', { name: 'Import collection' })).toBeEnabled();
  expect(screen.getByText('{{baseUrl}}/pets')).toBeVisible();
});

it('finds and renames variables beyond the first hundred by name or origin', async () => {
  const user = userEvent.setup();
  const variables = Array.from({ length: 101 }, (_, i) => ({
    name: `query_${i}`,
    sourceName: `query parameter: ${i}`,
    hasDefault: false,
  }));
  variables.push({ name: 'tenant', sourceName: 'header parameter: X-Tenant', hasDefault: false });
  render(
    <ImportDialog
      preview={{ ...preview, variables }}
      projectName="Project"
      busy={false}
      started={false}
      error={null}
      onRename={vi.fn(async () => true)}
      onImport={vi.fn()}
      onCancel={vi.fn()}
    />,
  );
  await user.click(screen.getByText('Show variable definitions'));
  expect(screen.queryByRole('button', { name: 'Rename tenant before import' })).toBeNull();
  await user.type(screen.getByRole('textbox', { name: 'Filter imported variables' }), 'X-Tenant');
  await user.click(screen.getByRole('button', { name: 'Rename tenant before import' }));
  expect(screen.getByRole('textbox', { name: 'Imported variable name' })).toHaveValue('tenant');
  expect(screen.getByRole('textbox', { name: 'Filter imported variables' })).toBeDisabled();
});
