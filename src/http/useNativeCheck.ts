import { useEffect, useRef, useState } from 'react';
import * as native from '../native';

/** Each explicit check owns a cancellable native reservation. Unmount discards late results. */
export function useNativeCheck<T>() {
  const [result, setResult] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const pending = useRef<{ id: string | null; cancelled: boolean } | null>(null);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      const operation = pending.current;
      if (operation) {
        operation.cancelled = true;
        if (operation.id) {
          void native.cancelHttpRequest(operation.id).catch(() => {});
        }
      }
    };
  }, []);
  async function run(action: (id: string) => Promise<T>) {
    if (pending.current) {
      return;
    }
    const operation = { id: null as string | null, cancelled: false };
    pending.current = operation;
    setRunning(true);
    setError(null);
    setResult(null);
    try {
      operation.id = await native.beginHttpRequest();
      if (operation.cancelled) {
        throw 'Check cancelled.';
      }
      const value = await action(operation.id);
      if (operation.cancelled) {
        throw 'Check cancelled.';
      }
      if (mounted.current && !operation.cancelled) {
        setResult(value);
      }
    } catch (error) {
      if (mounted.current) {
        setError(
          operation.cancelled
            ? 'Check cancelled.'
            : typeof error === 'string'
              ? error
              : 'Could not complete this check. Retry explicitly.',
        );
      }
    } finally {
      if (operation.id) {
        void native.cancelHttpRequest(operation.id).catch(() => {});
      }
      if (pending.current === operation) {
        pending.current = null;
      }
      if (mounted.current) {
        setRunning(false);
      }
    }
  }
  function cancel() {
    const operation = pending.current;
    if (!operation) {
      return;
    }
    operation.cancelled = true;
    if (operation.id) {
      void native.cancelHttpRequest(operation.id).catch(() => {
        if (mounted.current) {
          setError('Could not confirm cancellation. Do not retry automatically.');
        }
      });
    }
  }
  return { result, error, running, run, cancel };
}
