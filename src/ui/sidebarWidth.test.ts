import { describe, expect, it } from 'vitest';
import { SIDEBAR_MIN_WIDTH, shownSidebarWidth } from './sidebarWidth';

describe('shownSidebarWidth', () => {
  it('keeps the sidebar between the minimum and half of the window', () => {
    expect(shownSidebarWidth(320, 1400)).toBe(320);
    expect(shownSidebarWidth(120, 1400)).toBe(SIDEBAR_MIN_WIDTH);
    expect(shownSidebarWidth(900, 1400)).toBe(700);
    expect(shownSidebarWidth(900, 1401)).toBe(700);
  });

  it('never goes below the minimum in a very narrow window', () => {
    expect(shownSidebarWidth(320, 300)).toBe(SIDEBAR_MIN_WIDTH);
  });
});
