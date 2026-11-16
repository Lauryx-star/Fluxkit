import { useSyncExternalStore } from 'react';

/** Width of the navigation sidebar without a remembered choice. */
export const DEFAULT_SIDEBAR_WIDTH = 248;
export const SIDEBAR_MIN_WIDTH = 200;
/** Keyboard step for the sidebar divider, in pixels. */
export const SIDEBAR_KEY_STEP = 16;

/**
 * The width the sidebar is shown at: the user's choice, at most half of the window. The choice
 * itself is kept, so the sidebar grows back when the window becomes wider again.
 */
export function shownSidebarWidth(preferred: number, viewport: number) {
  const max = Math.max(SIDEBAR_MIN_WIDTH, Math.floor(viewport / 2));
  return Math.round(Math.min(max, Math.max(SIDEBAR_MIN_WIDTH, preferred)));
}

function subscribe(onChange: () => void) {
  window.addEventListener('resize', onChange);
  return () => window.removeEventListener('resize', onChange);
}

/** The current window width; follows window resizes. */
export function useViewportWidth() {
  return useSyncExternalStore(subscribe, () => window.innerWidth);
}
