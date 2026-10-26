import { useEffect, useRef, useState } from 'react';
import { useQueries } from '@tanstack/react-query';
import { ContextMenu, DropdownMenu } from './ui/context-menu';
import {
  FileImage,
  FolderOpen,
  Grid2X2,
  Grid3X3,
  MoreHorizontal,
  Plus,
  Search,
} from 'lucide-react';
import { useEditor } from '../editorController';
import type { ProjectPreview, RecentProject } from '../generated/bindings';
import pixauroMark from '../assets/pixauro-mark.svg';
import { Button } from './ui/button';

type ThumbnailSize = 'small' | 'large';
const preferenceKey = 'pixauro.thumbSize';
const relativeTime = new Intl.RelativeTimeFormat('en', { numeric: 'auto' });

function editedTime(timestamp: number | null | undefined, now: number) {
  if (timestamp == null) return 'Edit time unavailable';
  const seconds = Math.max(0, Math.floor((now - timestamp) / 1000));
  if (seconds < 60) return 'Edited just now';
  for (const [unit, duration] of [
    ['year', 31536000],
    ['month', 2592000],
    ['day', 86400],
    ['hour', 3600],
    ['minute', 60],
  ] as const) {
    if (seconds >= duration)
      return `Edited ${relativeTime.format(-Math.floor(seconds / duration), unit)}`;
  }
  return 'Edited just now';
}

function Thumbnail({ preview, size }: { preview: ProjectPreview; size: ThumbnailSize }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const target = size === 'small' ? 128 : 192;
  const longest = Math.max(preview.width, preview.height);
  // Fit every canvas to the selected size, including fractional upscales.
  const scale = target / longest;
  useEffect(() => {
    const context = canvas.current?.getContext('2d');
    if (!context) return;
    const data = context.createImageData(preview.previewWidth, preview.previewHeight);
    data.data.set(preview.rgba);
    context.putImageData(data, 0, 0);
  }, [preview]);
  return (
    <span
      className="project-checker"
      style={{
        width: Math.max(1, Math.round(preview.width * scale)),
        height: Math.max(1, Math.round(preview.height * scale)),
        backgroundSize: `${4 * scale}px ${4 * scale}px`,
      }}
    >
      <canvas
        ref={canvas}
        width={preview.previewWidth}
        height={preview.previewHeight}
        aria-hidden="true"
      />
    </span>
  );
}

function ProjectActions({ project, context }: { project: RecentProject; context?: boolean }) {
  const editor = useEditor();
  const Menu = context ? ContextMenu : DropdownMenu;
  return (
    <>
      <Menu.Item
        disabled={editor.loading}
        onSelect={() => void editor.revealRecent(project.id, project.path)}
      >
        {navigator.platform.startsWith('Mac') ? 'Show in Finder' : 'Show in file manager'}
      </Menu.Item>
      <Menu.Item
        disabled={editor.loading}
        onSelect={() => void editor.duplicateRecent(project.id, project.path)}
      >
        Duplicate
      </Menu.Item>
      <Menu.Separator />
      <Menu.Item
        disabled={editor.loading}
        onSelect={() => void editor.forgetRecent(project.id, project.path)}
      >
        Remove from recents
      </Menu.Item>
    </>
  );
}

function ProjectCard({
  project,
  preview,
  failed,
  size,
  now,
}: {
  project: RecentProject;
  preview?: ProjectPreview;
  failed: boolean;
  size: ThumbnailSize;
  now: number;
}) {
  const editor = useEditor();
  const [dropdownOpen, setDropdownOpen] = useState(false);
  const [contextOpen, setContextOpen] = useState(false);
  const name = preview?.name ?? project.name;
  return (
    <ContextMenu.Root onOpenChange={setContextOpen}>
      <ContextMenu.Trigger asChild disabled={editor.loading}>
        <article className={`project-card${dropdownOpen || contextOpen ? ' is-menu-open' : ''}`}>
          <button
            className="project-open"
            title={project.path}
            disabled={editor.loading || !editor.desktop}
            onClick={() => void editor.openProject(project.path, project.id)}
          >
            <span className="project-thumb">
              {preview ? (
                <Thumbnail preview={preview} size={size} />
              ) : (
                <span className="project-placeholder">
                  <FileImage aria-hidden="true" />
                  <span>{failed ? 'Preview unavailable' : 'Loading preview…'}</span>
                </span>
              )}
            </span>
            <span className="project-meta">
              <span className="project-meta-row">
                <span className="project-name">{name}</span>
                <span className="project-dim project-size">
                  {preview ? `${preview.width} × ${preview.height}` : '—'}
                </span>
              </span>
              <span className="project-dim">{editedTime(preview?.updatedAt, now)}</span>
            </span>
          </button>
          <DropdownMenu.Root open={dropdownOpen} onOpenChange={setDropdownOpen}>
            <DropdownMenu.Trigger asChild>
              <button
                className="project-more"
                disabled={editor.loading}
                aria-label={`Actions for ${name}`}
                title="Project actions"
              >
                <MoreHorizontal size={18} aria-hidden="true" />
              </button>
            </DropdownMenu.Trigger>
            <DropdownMenu.Content align="end">
              <ProjectActions project={project} />
            </DropdownMenu.Content>
          </DropdownMenu.Root>
        </article>
      </ContextMenu.Trigger>
      <ContextMenu.Content>
        <ProjectActions project={project} context />
      </ContextMenu.Content>
    </ContextMenu.Root>
  );
}

export function HomePage() {
  const editor = useEditor();
  const [search, setSearch] = useState('');
  const [sort, setSort] = useState<'recent' | 'name'>('recent');
  const [size, setSize] = useState<ThumbnailSize>(() => {
    try {
      return localStorage.getItem(preferenceKey) === 'large' ? 'large' : 'small';
    } catch {
      return 'small';
    }
  });
  const [now, setNow] = useState(Date.now);
  const searchInput = useRef<HTMLInputElement>(null);
  const previews = useQueries({
    queries: editor.recent.map((project) => ({
      queryKey: ['project-preview', project.id, project.path],
      queryFn: ({ signal }: { signal: AbortSignal }) =>
        editor.projectPreview(project.id, project.path, signal),
      enabled: editor.desktop,
      staleTime: 0,
      gcTime: 0,
      refetchOnWindowFocus: true,
    })),
  });
  useEffect(() => {
    try {
      localStorage.setItem(preferenceKey, size);
    } catch {
      /* Optional preference. */
    }
  }, [size]);
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 60000);
    const keydown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.ctrlKey || event.metaKey || event.altKey) return;
      const target = event.target;
      if (
        !(target instanceof HTMLElement) ||
        target.closest('input, textarea, select, [role="menu"], [role="dialog"], dialog') ||
        target.isContentEditable
      )
        return;
      if (event.key === '/') {
        event.preventDefault();
        searchInput.current?.focus();
      }
      if (event.key === 'Escape') setSearch('');
    };
    window.addEventListener('keydown', keydown);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener('keydown', keydown);
    };
  }, []);
  const projects = editor.recent
    .map((project, index) => ({
      project,
      preview: previews[index].data,
      failed: previews[index].isError,
    }))
    .filter(({ project, preview }) =>
      (preview?.name ?? project.name)
        .toLocaleLowerCase()
        .includes(search.trim().toLocaleLowerCase()),
    )
    .sort((a, b) =>
      sort === 'name'
        ? (a.preview?.name ?? a.project.name).localeCompare(
            b.preview?.name ?? b.project.name,
            undefined,
            { numeric: true },
          )
        : (b.preview?.updatedAt ?? 0) - (a.preview?.updatedAt ?? 0),
    );
  const disabled = editor.loading || !editor.desktop;
  const openButton = (
    <Button
      variant="ghost"
      disabled={disabled}
      onClick={() => void editor.openProject()}
      title="Open (⌘/Ctrl O)"
    >
      <FolderOpen aria-hidden="true" />
      Open…
    </Button>
  );
  return (
    <div className="home-scroll">
      <div className="home-content">
        {editor.loading ? (
          <p className="empty-message" role="status">
            Loading your workspace…
          </p>
        ) : editor.loadError ? (
          <p className="empty-message">
            Could not load projects. Use Retry above to reload your workspace.
          </p>
        ) : editor.recent.length === 0 ? (
          <section className="home-empty">
            <img src={pixauroMark} width={54} height={54} alt="" />
            <h1>No projects yet</h1>
            <p>Create a canvas or open a file.</p>
            <div className="home-empty-actions">
              <Button
                variant="default"
                disabled={disabled}
                onClick={() => void editor.newProject()}
              >
                <Plus aria-hidden="true" />
                New
              </Button>
              {openButton}
            </div>
          </section>
        ) : (
          <section aria-labelledby="projects-title">
            <div className="home-bar">
              <div className="home-title">
                <h1 id="projects-title">Projects</h1>
                <span className="muted" aria-live="polite">
                  {search.trim()
                    ? `${projects.length} / ${editor.recent.length}`
                    : editor.recent.length}
                </span>
              </div>
              <div className="home-tools">
                <label className="home-search">
                  <Search size={14} aria-hidden="true" />
                  <input
                    ref={searchInput}
                    type="search"
                    placeholder="Search"
                    aria-label="Search projects"
                    autoComplete="off"
                    autoCorrect="off"
                    autoCapitalize="none"
                    spellCheck={false}
                    value={search}
                    onChange={(event) => setSearch(event.target.value)}
                    onKeyDown={(event) => {
                      if (event.key === 'Escape') {
                        event.preventDefault();
                        setSearch('');
                      }
                    }}
                  />
                </label>
                <div className="home-segmented" role="group" aria-label="Sort projects">
                  <button aria-pressed={sort === 'recent'} onClick={() => setSort('recent')}>
                    Recent
                  </button>
                  <button aria-pressed={sort === 'name'} onClick={() => setSort('name')}>
                    Name
                  </button>
                </div>
                <div className="home-segmented" role="group" aria-label="Thumbnail size">
                  <button
                    className="icon"
                    aria-label="Small thumbnails"
                    aria-pressed={size === 'small'}
                    onClick={() => setSize('small')}
                  >
                    <Grid3X3 size={14} aria-hidden="true" />
                  </button>
                  <button
                    className="icon"
                    aria-label="Large thumbnails"
                    aria-pressed={size === 'large'}
                    onClick={() => setSize('large')}
                  >
                    <Grid2X2 size={14} aria-hidden="true" />
                  </button>
                </div>
                {openButton}
              </div>
            </div>
            <div className={`project-grid${size === 'large' ? ' is-large' : ''}`}>
              {projects.map(({ project, preview, failed }) => (
                <ProjectCard
                  key={`${project.id}:${project.path}`}
                  project={project}
                  preview={preview}
                  failed={failed}
                  size={size}
                  now={now}
                />
              ))}
              <article className="project-card new-project">
                <button
                  className="project-open"
                  disabled={disabled}
                  onClick={() => void editor.newProject()}
                  title="New project (⌘/Ctrl N)"
                >
                  <span className="project-thumb">
                    <Plus size={28} strokeWidth={1.5} aria-hidden="true" />
                  </span>
                  <span className="project-meta">
                    <span className="project-name">New project</span>
                    <span className="project-dim">Blank 32 × 32 canvas</span>
                  </span>
                </button>
              </article>
            </div>
            {projects.length === 0 && (
              <p className="home-no-match" role="status">
                No projects match “{search}”.
              </p>
            )}
          </section>
        )}
      </div>
    </div>
  );
}
