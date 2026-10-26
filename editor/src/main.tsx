import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClientProvider } from '@tanstack/react-query';
import {
  createRootRoute,
  createRoute,
  createRouter,
  createHashHistory,
  RouterProvider,
} from '@tanstack/react-router';
import { EditorProvider, queryClient } from './editorController';
import { AppShell } from './App';
import { HomePage } from './components/HomePage';
import { Workspace } from './Workspace';
import { SettingsPage } from './components/SettingsPage';
import 'dockview-react/dist/styles/dockview.css';
import './tokens.css';
import './styles.css';
import { TooltipProvider } from './components/ui/tooltip';

const rootRoute = createRootRoute({ component: AppShell });
const homeRoute = createRoute({ getParentRoute: () => rootRoute, path: '/', component: HomePage });
const editorRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/editor',
  component: Workspace,
});
const settingsRoute = createRoute({
  getParentRoute: () => editorRoute,
  path: 'settings',
  component: SettingsPage,
});
const router = createRouter({
  routeTree: rootRoute.addChildren([homeRoute, editorRoute.addChildren([settingsRoute])]),
  history: createHashHistory(),
});
declare module '@tanstack/react-router' {
  interface Register {
    router: typeof router;
  }
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <TooltipProvider delayDuration={400}>
        <EditorProvider>
          <RouterProvider router={router} />
        </EditorProvider>
      </TooltipProvider>
    </QueryClientProvider>
  </StrictMode>,
);
