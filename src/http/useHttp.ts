import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import * as native from '../native';
import type { Draft } from '../projects/types';
import { requestData } from '../projects/types';
import type { ExecutionResult, ExecutionScope } from './types';

interface Pending {
  id: string | null;
  cancelled: boolean;
}
interface RequestState {
  environmentLabel?: string;
  running: boolean;
  result: ExecutionResult | null;
}
const failure = (outcome: ExecutionResult['outcome'], message: string): ExecutionResult => ({
  outcome,
  message,
  status: null,
  elapsedMs: null,
  headers: [],
  byteCount: 0,
  text: null,
  jsonText: null,
  contentKind: 'empty',
  truncated: false,
  saved: false,
});

/** Native work is scoped to draft identities; late results never replace another draft's response. */
export function useHttp(drafts: Draft[]) {
  const [states, setStates] = useState<Record<number, RequestState>>({});
  const identity = drafts.map((draft) => draft.id).join(',');
  const [knownIdentity, setKnownIdentity] = useState(identity);
  if (knownIdentity !== identity) {
    setKnownIdentity(identity);
    const open = new Set(drafts.map((draft) => draft.id));
    setStates((previous) =>
      Object.fromEntries(Object.entries(previous).filter(([key]) => open.has(Number(key)))),
    );
  }

  const pending = useRef(new Map<number, Pending>());
  const present = useRef(new Set<number>());
  const mounted = useRef(true);
  useLayoutEffect(() => {
    present.current = new Set(drafts.map((draft) => draft.id));
    for (const [id, operation] of pending.current) {
      if (!present.current.has(id)) {
        operation.cancelled = true;
        if (operation.id) {
          void native.cancelHttpRequest(operation.id).catch(() => {});
        }
        pending.current.delete(id);
      }
    }
  }, [drafts]);
  useEffect(() => {
    mounted.current = true;
    const active = pending.current;
    return () => {
      mounted.current = false;
      for (const operation of active.values()) {
        operation.cancelled = true;
        if (operation.id) {
          void native.cancelHttpRequest(operation.id).catch(() => {});
        }
      }
    };
  }, []);
  function update(id: number, state: RequestState) {
    if (!mounted.current || !present.current.has(id)) {
      return;
    }
    setStates((previous) => {
      const next = Object.fromEntries(
        Object.entries(previous).filter(([key]) => present.current.has(Number(key))),
      );
      next[id] = state;
      // Keep at most eight complete response payloads. Metadata can remain with open drafts.
      const payloads = Object.keys(next).filter((key) => {
        const result = next[Number(key)]?.result;
        return (
          result && (result.text !== null || result.jsonText !== null || result.headers.length > 0)
        );
      });
      for (const key of payloads
        .filter((key) => Number(key) !== id)
        .slice(0, Math.max(0, payloads.length - 8))) {
        const old = next[Number(key)];
        if (old?.result) {
          next[Number(key)] = {
            ...old,
            result: {
              ...old.result,
              text: null,
              jsonText: null,
              headers: [],
              message:
                'Response details released to limit memory. Explicitly send again if you need a new response.',
            },
          };
        }
      }
      return next;
    });
  }
  async function send(
    draft: Draft,
    scope: ExecutionScope | null,
    timeoutSeconds: number,
    saveResponse: boolean,
    environmentLabel = 'Session',
  ) {
    if (pending.current.has(draft.id)) {
      return;
    }
    const operation: Pending = { id: null, cancelled: false };
    pending.current.set(draft.id, operation);
    update(draft.id, { running: true, result: null, environmentLabel });
    try {
      operation.id = await native.beginHttpRequest();
      if (operation.cancelled) {
        await native.cancelHttpRequest(operation.id);
        update(draft.id, {
          running: false,
          result: failure('cancelled', 'Cancelled before dispatch. No HTTP request was sent.'),
        });
        return;
      }
      const result = await native.executeHttpRequest(operation.id, {
        request: requestData(draft),
        scope,
        timeoutMs: timeoutSeconds * 1000,
        saveResponse,
      });
      update(draft.id, { running: false, result, environmentLabel });
    } catch (error) {
      update(draft.id, {
        running: false,
        result: failure(
          operation.cancelled ? 'cancelled' : 'invalidRequest',
          operation.cancelled
            ? 'Request cancelled.'
            : typeof error === 'string'
              ? error
              : 'Could not start the native HTTP request.',
        ),
      });
    } finally {
      if (operation.id) {
        void native.cancelHttpRequest(operation.id).catch(() => {});
      }
      if (pending.current.get(draft.id) === operation) {
        pending.current.delete(draft.id);
      }
    }
  }
  async function cancel(id: number) {
    const operation = pending.current.get(id);
    if (!operation) {
      return;
    }
    operation.cancelled = true;
    if (operation.id) {
      try {
        await native.cancelHttpRequest(operation.id);
      } catch {
        update(id, {
          running: true,
          result: failure(
            'networkError',
            'Could not confirm cancellation. The request may still be running; do not resend automatically.',
          ),
        });
      }
    }
  }
  return { states, send, cancel };
}
