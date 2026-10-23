import { useContext, useEffect, useId, useLayoutEffect, useRef, useState } from 'react';
import { environmentAction } from '../native';
import { VariableContext } from './VariableContext';

interface Props {
  label: string;
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  variant?: 'url' | 'pair' | 'body';
}

/** Native text editing with a synchronized, non-interactive syntax mirror. */
export function VariableInput({ label, value, onChange, placeholder, variant = 'pair' }: Props) {
  const context = useContext(VariableContext);
  const input = useRef<HTMLInputElement | HTMLTextAreaElement>(null);
  const mirror = useRef<HTMLDivElement>(null);
  const popup = useRef<HTMLUListElement>(null);
  const id = useId();
  const [focused, setFocused] = useState(false);
  const [composing, setComposing] = useState(false);
  const [caret, setCaret] = useState({ start: 0, end: 0 });
  const [active, setActive] = useState(0);
  const [dismissed, setDismissed] = useState('');
  const previewId = useRef(0);
  const [hover, setHover] = useState<{
    name: string;
    context: typeof context;
    template: string;
    message: string;
    state: 'loading' | 'value' | 'error';
  } | null>(null);
  const prefix = value.slice(0, caret.start).match(/\{\{([A-Za-z0-9_.-]*)$/);
  const query = prefix?.[1];
  const completionKey = `${value}:${caret.start}:${caret.end}`;
  const completing =
    focused &&
    !composing &&
    query !== undefined &&
    caret.start === caret.end &&
    dismissed !== completionKey;
  const candidates =
    context.view?.data.variables.filter((item) =>
      item.name.toLowerCase().includes((query ?? '').toLowerCase()),
    ) ?? [];
  const matches = candidates.slice(0, 20);
  const showSuggestions = completing && context.view !== null;
  const selected = Math.min(active, Math.max(0, matches.length - 1));
  const tooltip =
    !showSuggestions && hover?.context === context && hover.template === value ? hover : null;

  function syncScroll() {
    if (mirror.current && input.current) {
      mirror.current.scrollLeft = input.current.scrollLeft;
      mirror.current.scrollTop = input.current.scrollTop;
    }
  }
  useLayoutEffect(syncScroll, [value, caret]);
  useEffect(() => {
    popup.current?.querySelector('[aria-selected="true"]')?.scrollIntoView?.({ block: 'nearest' });
  }, [selected]);
  useEffect(
    () => () => {
      previewId.current += 1;
    },
    [context, value],
  );

  function selection() {
    if (input.current) {
      setCaret({ start: input.current.selectionStart ?? 0, end: input.current.selectionEnd ?? 0 });
    }
  }
  function hidePreview() {
    previewId.current += 1;
    setHover(null);
  }
  function preview(name: string) {
    if (hover?.name === name && hover.context === context && hover.template === value) {
      return;
    }
    const ticket = ++previewId.current;
    const snapshot = { name, context, template: value };
    const definition = context.view?.data.variables.find((item) => item.name === name);
    if (!context.handle || !context.view || !definition) {
      setHover({
        ...snapshot,
        state: 'error',
        message: context.view
          ? 'This variable is not defined in the current project.'
          : 'Open a desktop project to preview variable values.',
      });
      return;
    }
    setHover({ ...snapshot, state: 'loading', message: 'Loading this variable…' });
    void environmentAction(context.handle, {
      kind: 'preview',
      variable: definition.id,
      environment: context.view.selectedEnvironment,
      expected: context.view.revision,
    }).then(
      (reply) => {
        if (ticket === previewId.current) {
          if (
            reply.view.revision !== context.view?.revision ||
            reply.view.selectedEnvironment !== context.view?.selectedEnvironment ||
            reply.view.selectionMissing
          ) {
            setHover({
              ...snapshot,
              state: 'error',
              message:
                'Project variables or environment changed. Reload variables before previewing values.',
            });
            return;
          }
          setHover({
            ...snapshot,
            state: 'value',
            message:
              reply.value === '' ? '(empty value)' : (reply.value ?? 'No value is available.'),
          });
        }
      },
      (error: unknown) => {
        if (ticket === previewId.current) {
          setHover({
            ...snapshot,
            state: 'error',
            message:
              typeof error === 'string'
                ? error
                : 'Could not read this variable from the OS credential store.',
          });
        }
      },
    );
  }
  function choose(name: string) {
    if (query === undefined) {
      return;
    }
    const start = caret.start - query.length - 2;
    const suffix = value.slice(caret.start).match(/^[A-Za-z0-9_.-]*(?:\}\})?/)?.[0] ?? '';
    const inserted = `{{${name}}}`;
    const next = value.slice(0, start) + inserted + value.slice(caret.start + suffix.length);
    onChange(next);
    setDismissed(`${next}:${start + inserted.length}:${start + inserted.length}`);
    setCaret({ start: start + inserted.length, end: start + inserted.length });
    hidePreview();
    requestAnimationFrame(() => {
      input.current?.focus();
      input.current?.setSelectionRange(start + inserted.length, start + inserted.length);
      syncScroll();
    });
  }
  const control = {
    'aria-label': label,
    'aria-autocomplete': 'list' as const,
    'aria-controls': showSuggestions ? `${id}-options` : undefined,
    'aria-activedescendant':
      showSuggestions && matches.length ? `${id}-option-${selected}` : undefined,
    'aria-describedby': tooltip ? `${id}-tooltip` : undefined,
    className: 'variable-control',
    value,
    placeholder,
    autoComplete: 'off',
    spellCheck: false,
    onFocus: () => {
      setFocused(true);
      selection();
    },
    onBlur: () => {
      setFocused(false);
      hidePreview();
    },
    onSelect: selection,
    onScroll: () => {
      syncScroll();
      hidePreview();
    },
    onCompositionStart: () => setComposing(true),
    onCompositionEnd: () => setComposing(false),
    onChange: (event: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) => {
      onChange(event.target.value);
      setCaret({ start: event.target.selectionStart ?? 0, end: event.target.selectionEnd ?? 0 });
      setActive(0);
      hidePreview();
    },
    onKeyDown: (event: React.KeyboardEvent<HTMLInputElement | HTMLTextAreaElement>) => {
      if (event.nativeEvent.isComposing) {
        return;
      }
      if (event.key === 'Escape') {
        setDismissed(completionKey);
        hidePreview();
        return;
      }
      if (!showSuggestions || matches.length === 0) {
        return;
      }
      if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault();
        setActive(
          (selected + (event.key === 'ArrowDown' ? 1 : matches.length - 1)) % matches.length,
        );
      } else if (event.key === 'Enter' || event.key === 'Tab') {
        event.preventDefault();
        const item = matches[selected];
        if (item) {
          choose(item.name);
        }
      }
    },
  };
  const parts: React.ReactNode[] = [];
  let previous = 0;
  let count = 0;
  let limited = false;
  const names = new Set(context.view?.data.variables.map((item) => item.name));
  for (const match of value.matchAll(/\{\{([A-Za-z_][A-Za-z0-9_.-]*)\}\}/g)) {
    if (count === 500) {
      limited = true;
      break;
    }
    count += 1;
    const name = match[1] ?? '';
    const known = names.has(name);
    parts.push(value.slice(previous, match.index));
    parts.push(
      <span
        key={match.index}
        data-variable={name}
        className={`variable-token ${known ? 'variable-known' : 'variable-unknown'}`}
      >
        {match[0]}
      </span>,
    );
    previous = match.index + match[0].length;
  }
  parts.push(value.slice(previous));
  return (
    <div
      className={`variable-field variable-field-${variant}`}
      onMouseLeave={hidePreview}
      onMouseMove={(event) => {
        if (
          showSuggestions ||
          (event.target as HTMLElement).closest('[role="tooltip"], [role="listbox"]')
        ) {
          return;
        }
        const token = Array.from(
          mirror.current?.querySelectorAll<HTMLElement>('[data-variable]') ?? [],
        ).find((element) =>
          Array.from(element.getClientRects()).some(
            (rect) =>
              event.clientX >= rect.left &&
              event.clientX <= rect.right &&
              event.clientY >= rect.top &&
              event.clientY <= rect.bottom,
          ),
        );
        if (token?.dataset.variable) {
          preview(token.dataset.variable);
        } else if (hover) {
          hidePreview();
        }
      }}
    >
      <div className="variable-surface">
        <div ref={mirror} className="variable-mirror" aria-hidden="true">
          {parts}
          {variant === 'body' ? '\n' : ''}
        </div>
        {variant === 'body' ? (
          <textarea
            {...control}
            ref={(node) => {
              input.current = node;
            }}
          />
        ) : (
          <input
            {...control}
            type="text"
            ref={(node) => {
              input.current = node;
            }}
          />
        )}
      </div>
      {limited && (
        <small className="variable-limit">
          Highlighting and hover previews are limited to the first 500 placeholders in this field.
          The full text is preserved.
        </small>
      )}
      {showSuggestions && (
        <div className="variable-suggestions">
          <ul ref={popup} role="listbox" aria-label={`Variables for ${label}`} id={`${id}-options`}>
            {matches.map((item, index) => (
              <li
                key={item.id}
                role="option"
                id={`${id}-option-${index}`}
                aria-selected={selected === index}
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => choose(item.name)}
              >
                <code>{`{{${item.name}}}`}</code>
              </li>
            ))}
          </ul>
          <small>
            {matches.length
              ? '↑ ↓ select · Enter / Tab insert · Esc close · Type to filter (up to 20 matches)'
              : 'No matching variables. Add one in Variables.'}
          </small>
        </div>
      )}
      {tooltip && (
        <div
          className={`variable-tooltip variable-tooltip-${tooltip.state}`}
          role="tooltip"
          id={`${id}-tooltip`}
        >
          <strong>{tooltip.name}</strong>
          <small>
            {context.view?.data.environments.find(
              (item) => item.id === context.view?.selectedEnvironment,
            )?.name ?? 'Defaults'}{' '}
            · shared default if absent
          </small>
          <span>{tooltip.message}</span>
        </div>
      )}
    </div>
  );
}
