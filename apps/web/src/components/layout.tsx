import { Outlet } from 'react-router';

import { Header } from '@/components/header';

export function Layout() {
  return (
    <div className="min-h-dvh">
      <Header />
      <main className="mx-auto w-full max-w-5xl px-4 py-8">
        <Outlet />
      </main>
      <footer className="mx-auto w-full max-w-5xl px-4 pb-10 text-xs text-muted-foreground">
        Open source, unaudited, and holding nothing. Read the risks before using
        anything but test funds.
      </footer>
    </div>
  );
}
