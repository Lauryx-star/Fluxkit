import { fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useState } from 'react';
import { expect, it, vi } from 'vitest';
import { Select } from './Select';
import { Modal } from '../projects/Dialogs';

function Example({ disabled = false }: { disabled?: boolean }) {
  const [value, setValue] = useState('get');
  return (
    <>
      <Select aria-label="Method" value={value} onValueChange={setValue} disabled={disabled}>
        <option value="get">GET</option>
        <>
          <option value="blocked" disabled>
            Unavailable
          </option>
          <option value="post">POST</option>
          <option value="put">PUT</option>
        </>
      </Select>
      <button>Next</button>
    </>
  );
}
it('opens a themed menu, selects by mouse, and closes on an outside click', async () => {
  const user = userEvent.setup();
  render(<Example />);
  const control = screen.getByRole('combobox', { name: 'Method' });
  expect(control).toHaveAttribute('aria-expanded', 'false');
  await user.click(control);
  const menu = screen.getByRole('listbox', { name: 'Method' });
  expect(menu).toHaveClass('select-menu');
  expect(within(menu).getByRole('option', { name: 'GET' })).toHaveAttribute(
    'aria-selected',
    'true',
  );
  await user.click(within(menu).getByRole('option', { name: 'Unavailable' }));
  expect(control).toHaveValue('get');
  await user.click(within(menu).getByRole('option', { name: 'POST' }));
  expect(control).toHaveValue('post');
  expect(screen.queryByRole('listbox')).toBeNull();
  expect(control).toHaveFocus();
  await user.click(control);
  await user.click(screen.getByRole('button', { name: 'Next' }));
  expect(screen.queryByRole('listbox')).toBeNull();
});
it('supports arrows, Home/End, typeahead and Enter without committing on Escape or Tab', async () => {
  const user = userEvent.setup();
  render(<Example />);
  const control = screen.getByRole('combobox');
  control.focus();
  await user.keyboard('{ArrowDown}{ArrowDown}{Enter}');
  expect(control).toHaveValue('post');
  await user.keyboard('{End}{Enter}');
  expect(control).toHaveValue('put');
  await user.keyboard('{Home}{Escape}');
  expect(control).toHaveValue('put');
  expect(screen.queryByRole('listbox')).toBeNull();
  await user.keyboard('po{Enter}');
  expect(control).toHaveValue('post');
  await user.keyboard('{ArrowDown}{ArrowUp}{Tab}');
  expect(control).toHaveValue('post');
  expect(screen.getByRole('button', { name: 'Next' })).toHaveFocus();
  expect(screen.queryByRole('listbox')).toBeNull();
});
it('keeps menus inside dialogs, handles Escape locally, and preserves form validation', async () => {
  const user = userEvent.setup();
  const escape = vi.fn();
  const submit = vi.fn((event: React.FormEvent) => event.preventDefault());
  render(
    <Modal label="Configuration">
      <form
        onSubmit={submit}
        onKeyDown={(e) => {
          if (e.key === 'Escape') {
            escape();
          }
        }}
      >
        <Select aria-label="Required value" name="value" value="" onValueChange={vi.fn()} required>
          <option value="">Choose a value</option>
          <option value="test">Test</option>
        </Select>
        <button type="submit">Save</button>
      </form>
    </Modal>,
  );
  await user.click(screen.getByRole('button', { name: 'Save' }));
  expect(submit).not.toHaveBeenCalled();
  const control = screen.getByRole('combobox');
  expect(control).toHaveFocus();
  expect(within(screen.getByRole('dialog')).getByRole('listbox')).toBeVisible();
  await user.keyboard('{Escape}');
  expect(escape).not.toHaveBeenCalled();
  await user.keyboard('{Escape}');
  expect(escape).toHaveBeenCalledOnce();
});
it('keeps disabled controls inert and exposes an accessible empty state', async () => {
  const user = userEvent.setup();
  const { rerender } = render(<Example disabled />);
  await user.click(screen.getByRole('combobox'));
  expect(screen.queryByRole('listbox')).toBeNull();
  rerender(
    <Select aria-label="Empty" value="" onValueChange={vi.fn()}>
      {[]}
    </Select>,
  );
  await user.click(screen.getByRole('combobox'));
  expect(screen.getByText('No options available')).toBeVisible();
  fireEvent.keyDown(screen.getByRole('combobox'), { key: 'ArrowDown' });
  expect(screen.getByRole('combobox')).not.toHaveAttribute('aria-activedescendant');
});
it('focuses only the first missing required selection and retains the chosen form value', async () => {
  const user = userEvent.setup();
  const submit = vi.fn((event: React.FormEvent) => event.preventDefault());
  function Form() {
    const [value, setValue] = useState('');
    return (
      <form aria-label="Values" onSubmit={submit}>
        <Select aria-label="First" name="first" required value={value} onValueChange={setValue}>
          <option value="">Choose</option>
          <option value="a">Alpha</option>
        </Select>
        <Select aria-label="Second" name="second" required value="" onValueChange={vi.fn()}>
          <option value="">Choose</option>
        </Select>
        <button type="submit">Save</button>
      </form>
    );
  }
  render(<Form />);
  await user.click(screen.getByRole('button', { name: 'Save' }));
  expect(screen.getByRole('combobox', { name: 'First' })).toHaveFocus();
  expect(screen.getAllByRole('listbox')).toHaveLength(1);
  await user.click(screen.getByRole('option', { name: 'Alpha' }));
  const form = screen.getByRole('form') as HTMLFormElement;
  expect(new FormData(form).get('first')).toBe('a');
  expect(submit).not.toHaveBeenCalled();
});
