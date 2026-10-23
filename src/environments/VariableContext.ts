import { createContext } from 'react';
import type { EnvironmentView } from './types';

/** Definitions only. Values are fetched by an explicit hover, never by the provider. */
export const VariableContext = createContext<{
  handle: string | null;
  view: EnvironmentView | null;
}>({ handle: null, view: null });
