import { useEffect, useState } from 'react';
import { SIDEBAR_KEY_STEP, SIDEBAR_MIN_WIDTH, shownSidebarWidth } from './sidebarWidth';

/** The vertical divider between the sidebar and the workspace. */
export function SidebarResizer(props: {
  width: number;
  viewport: number;
  onResize: (width: number) => void;
}) {
  const [dragging, setDragging] = useState(false);
  const max = shownSidebarWidth(Number.MAX_SAFE_INTEGER, props.viewport);
  useEffect(() => {
    if (!dragging) {
      return;
    }
    const previousCursor = document.body.style.cursor;
    const previousUserSelect = document.body.style.userSelect;
    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';
    return () => {
      document.body.style.cursor = previousCursor;
      document.body.style.userSelect = previousUserSelect;
    };
  }, [dragging]);
  function fromPointer(element: HTMLElement, clientX: number) {
    const left = element.previousElementSibling?.getBoundingClientRect().left ?? 0;
    props.onResize(shownSidebarWidth(clientX - left, props.viewport));
  }
  return (
    <div
      className={`sidebar-resizer${dragging ? ' is-dragging' : ''}`}
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize sidebar"
      aria-valuenow={props.width}
      aria-valuemin={SIDEBAR_MIN_WIDTH}
      aria-valuemax={max}
      tabIndex={0}
      onPointerDown={(event) => {
        event.currentTarget.setPointerCapture(event.pointerId);
        setDragging(true);
      }}
      onPointerMove={(event) => {
        if (event.currentTarget.hasPointerCapture(event.pointerId)) {
          fromPointer(event.currentTarget, event.clientX);
        }
      }}
      onPointerUp={(event) => {
        event.currentTarget.releasePointerCapture(event.pointerId);
        setDragging(false);
      }}
      onPointerCancel={() => setDragging(false)}
      onKeyDown={(event) => {
        const next = {
          ArrowLeft: props.width - SIDEBAR_KEY_STEP,
          ArrowRight: props.width + SIDEBAR_KEY_STEP,
          Home: SIDEBAR_MIN_WIDTH,
          End: max,
        }[event.key];
        if (next !== undefined) {
          event.preventDefault();
          props.onResize(shownSidebarWidth(next, props.viewport));
        }
      }}
    />
  );
}
