// First, and deliberately: it installs a global the Solana libraries expect at
// module-initialisation time.
import './polyfill';

import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { RouterProvider, createBrowserRouter } from 'react-router';

import { ErrorBoundary } from '@/components/error-boundary';
import { Layout } from '@/components/layout';
import { SolanaProviders } from '@/components/solana-providers';
import { CreateRoute } from '@/routes/create';
import { MarketRoute } from '@/routes/market';
import { MarketsRoute } from '@/routes/markets';
import { NotFoundRoute } from '@/routes/not-found';

import './globals.css';

const router = createBrowserRouter([
  {
    element: <Layout />,
    children: [
      { path: '/', element: <MarketsRoute /> },
      { path: '/markets/:market', element: <MarketRoute /> },
      { path: '/create', element: <CreateRoute /> },
      { path: '*', element: <NotFoundRoute /> },
    ],
  },
]);

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <ErrorBoundary>
      <SolanaProviders>
        <RouterProvider router={router} />
      </SolanaProviders>
    </ErrorBoundary>
  </StrictMode>,
);
