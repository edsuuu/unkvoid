import { createContext } from 'react';

import type { App } from '../core/App.ts';

export const AppContext = createContext<App | null>(null);
