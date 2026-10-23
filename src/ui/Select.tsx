import {
  Children,
  Fragment,
  isValidElement,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
  type AriaAttributes,
} from 'react';
import { createPortal } from 'react-dom';

type Option = { value: string; label: string; disabled: boolean };
function text(children: ReactNode): string {
  return Children.toArray(children)
    .map((child) =>
      isValidElement<{ children?: ReactNode }>(child) ? text(child.props.children) : String(child),
    )
    .join('');
}
function optionsFrom(children: ReactNode): Option[] {
  return Children.toArray(children).flatMap((child) => {
    if (!isValidElement<{ children?: ReactNode; value?: string; disabled?: boolean }>(child)) {
      return [];
    }
    if (child.type === Fragment) {
      return optionsFrom(child.props.children);
    }
    if (child.type !== 'option') {
      return [];
    }
    const label = text(child.props.children);
    return [{ value: child.props.value ?? label, label, disabled: !!child.props.disabled }];
  });
}

/** A select-only combobox. Option children are definitions, never native popup menus. */
export function Select({
  value,
  onValueChange,
  children,
  disabled = false,
  required = false,
  name,
  id,
  className,
  'aria-label': label,
  'aria-labelledby': labelledBy,
  'aria-describedby': describedBy,
}: Pick<AriaAttributes, 'aria-label' | 'aria-labelledby' | 'aria-describedby'> & {
  value: string;
  onValueChange: (value: string) => void;
  children: ReactNode;
  disabled?: boolean;
  required?: boolean;
  name?: string;
  id?: string;
  className?: string;
}) {
  const uid = useId();
  const listId = `${uid}-options`;
  const trigger = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const search = useRef({ text: '', time: 0 });
  const [open, setOpen] = useState(false);
  const [portalTarget, setPortalTarget] = useState<Element | null>(null);
  const [active, setActive] = useState(-1);
  const [position, setPosition] = useState({ left: 0, top: 0, width: 0, maxHeight: 280 });
  const options = optionsFrom(children);
  const selected = options.findIndex((option) => option.value === value);
  const expanded = open && !disabled;

  function show(index = selected) {
    if (disabled) {
      return;
    }
    // Safari does not focus buttons on mouse click; keep keyboard control on the trigger.
    trigger.current?.focus();
    search.current = { text: '', time: 0 };
    setPortalTarget(trigger.current?.closest('[role="dialog"]') ?? document.body);
    setActive(
      index >= 0 && !options[index]?.disabled ? index : options.findIndex((o) => !o.disabled),
    );
    setOpen(true);
  }
  function choose(index: number) {
    const option = options[index];
    if (!option || option.disabled) {
      return;
    }
    setOpen(false);
    if (option.value !== value) {
      onValueChange(option.value);
    }
  }
  useLayoutEffect(() => {
    if (!expanded) {
      return;
    }
    function place() {
      const rect = trigger.current?.getBoundingClientRect();
      if (!rect) {
        return;
      }
      const below = window.innerHeight - rect.bottom - 12;
      const above = rect.top - 12;
      const upward = below < 220 && above > below;
      const maxHeight = Math.max(40, Math.min(280, upward ? above : below));
      const height = Math.min(list.current?.scrollHeight ?? maxHeight, maxHeight);
      const width = Math.min(Math.max(rect.width, 220), window.innerWidth - 16);
      setPosition({
        left: Math.max(8, Math.min(rect.left, window.innerWidth - width - 8)),
        top: upward ? rect.top - height - 6 : rect.bottom + 6,
        width,
        maxHeight,
      });
    }
    function outside(event: Event) {
      if (
        event.target instanceof Node &&
        !trigger.current?.contains(event.target) &&
        !list.current?.contains(event.target)
      ) {
        setOpen(false);
      }
    }
    place();
    window.addEventListener('resize', place);
    window.addEventListener('scroll', place, true);
    document.addEventListener('pointerdown', outside, true);
    document.addEventListener('focusin', outside);
    return () => {
      window.removeEventListener('resize', place);
      window.removeEventListener('scroll', place, true);
      document.removeEventListener('pointerdown', outside, true);
      document.removeEventListener('focusin', outside);
    };
  }, [expanded]);
  useEffect(() => {
    if (expanded) {
      document.getElementById(`${listId}-${active}`)?.scrollIntoView?.({ block: 'nearest' });
    }
  }, [active, expanded, listId]);

  return (
    <>
      <button
        ref={trigger}
        id={id}
        type="button"
        role="combobox"
        className={`select-control ${className ?? ''}`}
        value={value}
        disabled={disabled}
        aria-label={label}
        aria-labelledby={labelledBy}
        aria-describedby={describedBy}
        aria-required={required || undefined}
        aria-haspopup="listbox"
        aria-expanded={expanded}
        aria-controls={expanded ? listId : undefined}
        aria-activedescendant={expanded && active >= 0 ? `${listId}-${active}` : undefined}
        onClick={() => (expanded ? setOpen(false) : show())}
        onKeyDown={(event) => {
          if (event.key === 'Tab') {
            setOpen(false);
            return;
          }
          if (event.key === 'Escape' && expanded) {
            event.preventDefault();
            event.stopPropagation();
            setOpen(false);
            return;
          }
          if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
            event.preventDefault();
            const enabled = options.flatMap((option, index) => (option.disabled ? [] : [index]));
            const current = enabled.indexOf(active);
            const next =
              event.key === 'Home'
                ? enabled[0]
                : event.key === 'End'
                  ? enabled.at(-1)
                  : !expanded
                    ? selected >= 0 && !options[selected]?.disabled
                      ? selected
                      : event.key === 'ArrowUp'
                        ? enabled.at(-1)
                        : enabled[0]
                    : enabled[
                        (current + (event.key === 'ArrowDown' ? 1 : -1) + enabled.length) %
                          enabled.length
                      ];
            show(next ?? -1);
          } else if (
            event.key === 'Enter' ||
            (event.key === ' ' && (!search.current.text || Date.now() - search.current.time >= 700))
          ) {
            event.preventDefault();
            if (expanded) {
              choose(active);
            } else {
              show();
            }
          } else if (event.key.length === 1 && !event.metaKey && !event.ctrlKey && !event.altKey) {
            event.preventDefault();
            const now = Date.now();
            const query =
              (now - search.current.time < 700 ? search.current.text : '') +
              event.key.toLocaleLowerCase();
            const repeated = [...query].every((char) => char === query[0]);
            const prefix = repeated ? query[0]! : query;
            const start = repeated ? (expanded ? active : selected) + 1 : 0;
            const match = options
              .map((_, i) => (start + i + options.length) % options.length)
              .find(
                (i) =>
                  !options[i]!.disabled && options[i]!.label.toLocaleLowerCase().startsWith(prefix),
              );
            if (!expanded) {
              show(match ?? selected);
            } else if (match !== undefined) {
              setActive(match);
            }
            search.current = { text: query, time: now };
          }
        }}
      >
        <span className="select-value">{options[selected]?.label ?? 'Choose…'}</span>
        <span className="select-chevron" aria-hidden="true">
          ⌄
        </span>
      </button>
      {/* Preserve native form submission and required-field validation without a native popup. */}
      <select
        className="select-form-value"
        aria-hidden="true"
        tabIndex={-1}
        name={name}
        value={value}
        disabled={disabled}
        required={required}
        onChange={(event) => onValueChange(event.target.value)}
        onInvalid={(event) => {
          event.preventDefault();
          const firstInvalid = Array.from(event.currentTarget.form?.elements ?? []).find(
            (control) =>
              (control instanceof HTMLInputElement ||
                control instanceof HTMLSelectElement ||
                control instanceof HTMLTextAreaElement) &&
              control.willValidate &&
              !control.validity.valid,
          );
          if (firstInvalid && firstInvalid !== event.currentTarget) {
            return;
          }
          trigger.current?.focus();
          show();
        }}
      >
        {children}
      </select>
      {expanded &&
        portalTarget &&
        createPortal(
          <div
            ref={list}
            id={listId}
            role="listbox"
            aria-label={label}
            aria-labelledby={labelledBy}
            className="select-menu"
            style={position}
            onMouseDown={(event) => event.preventDefault()}
          >
            {options.map((option, index) => (
              <div
                key={option.value}
                id={`${listId}-${index}`}
                role="option"
                aria-selected={option.value === value}
                aria-disabled={option.disabled || undefined}
                data-value={option.value}
                className={`select-option${index === active ? ' is-active' : ''}`}
                onPointerMove={() => {
                  if (!option.disabled) {
                    setActive(index);
                  }
                }}
                onClick={(event) => {
                  event.stopPropagation();
                  choose(index);
                }}
              >
                <span>{option.label}</span>
                {option.value === value && <span aria-hidden="true">✓</span>}
              </div>
            ))}
            {!options.length && <div className="select-empty">No options available</div>}
          </div>,
          portalTarget,
        )}
    </>
  );
}
