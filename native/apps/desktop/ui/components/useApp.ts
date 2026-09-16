import { useContext } from 'react';

import type { App } from '../core/App.ts';
import { AppContext } from './AppContext.ts';

export function useApp(): App {
    const app = useContext(AppContext);

    if (! app) {
        throw new Error('a interface foi montada fora do AppContext');
    }

    return app;
}
