import { Outlet } from 'react-router';

import { Header } from '@/components/header';

export function Layout() {
  return (
    <div className="min-h-dvh">
      <Header />
      <main className="mx-auto w-full max-w-5xl px-4 py-8">
        <Outlet />
      </main>
    </div>
  );
}
