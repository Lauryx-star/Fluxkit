import { screen, within } from '@testing-library/react';
import type userEvent from '@testing-library/user-event';

/** Exercise the custom menu through the same clicks a desktop user makes. */
export async function selectOption(
  user: ReturnType<typeof userEvent.setup>,
  control: HTMLElement,
  value: string,
) {
  if (control.getAttribute('aria-expanded') !== 'true') {
    await user.click(control);
  }
  const menu = screen.getByRole('listbox');
  const option = within(menu)
    .getAllByRole('option')
    .find((item) => item.dataset.value === value);
  if (!option) {
    throw new Error(`Missing option: ${value}`);
  }
  await user.click(option);
}
