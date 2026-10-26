import { createContext, useContext, useEffect, useMemo, type ReactNode } from 'react';
import { QueryClient, useMutation, useQuery } from '@tanstack/react-query';
import { isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import {
  commands,
  type EditorSnapshot,
  type DocumentGuard,
  type DocumentSnapshot,
  type NewProjectOptions,
  type LayerEdit,
  type AnimationEdit,
  type LayerSnapshot,
  type PngImportOptions,
  type SpritePreview,
  type SpriteSlice,
  type ImportAssembly,
} from './generated/bindings';
import { reconcileInspection } from './inspection';
import { animationDocument, selectedAnimation, celAt } from './animation';
import {
  useUi,
  type RGBA,
  type Stroke,
  type LayerOpacityPreview,
  type CelMove,
} from './editorUiState';
import { selectionMatches } from './selection';
import { pickFrameColor } from './colorPicker';

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: false,
      staleTime: Infinity,
      refetchOnWindowFocus: false,
      networkMode: 'always',
    },
    mutations: { retry: false, networkMode: 'always' },
  },
});
const queryKey = ['editor-snapshot'] as const;
const desktop = isTauri();
const empty: EditorSnapshot = { document: null, recent: [], warning: null };
let lane: Promise<unknown> = Promise.resolve();
let strokeFlusher: (() => Promise<boolean>) | null = null;
let flow: Promise<boolean> | null = null;
let closing: Promise<boolean> | null = null;
let opacityCommit: Promise<boolean> | null = null;
let opacityFrame: number | null = null;
let queuedOpacity: LayerOpacityPreview | null = null;
type RunningImport = { id: string; cancelled: boolean; promise: Promise<boolean> | null };
let runningImport: RunningImport | null = null;

async function cancelPngImport() {
  const job = runningImport;
  if (!job) return;
  job.cancelled = true;
  useUi.getState().set({ importCancelling: true });
  try {
    unwrap(await commands.cancelPngImport(job.id));
  } catch (error) {
    useUi
      .getState()
      .set({ error: `Could not cancel import: ${message(error)}`, importCancelling: false });
  }
}

async function pollPngImport(job: RunningImport) {
  if (runningImport !== job) return;
  try {
    const progress = unwrap(await commands.pngImportStatus(job.id));
    if (runningImport !== job) return;
    if (progress) {
      useUi.getState().set({ importProgress: progress });
      // A cancel can arrive while the import is still waiting to enter the IPC lane.
      if (job.cancelled && progress.stage !== 'committing')
        unwrap(await commands.cancelPngImport(job.id));
    }
  } catch {
    // Keep the worker running; the import command reports authoritative errors.
    if (runningImport === job) useUi.getState().set({ importProgress: null });
  }
  if (runningImport === job) setTimeout(() => void pollPngImport(job), 200);
}

function discardQueuedOpacity() {
  if (opacityFrame !== null) cancelAnimationFrame(opacityFrame);
  opacityFrame = null;
  queuedOpacity = null;
}

/** Publish at most once per frame; flushing before a command preserves the final input. */
function publishQueuedOpacity() {
  const preview = queuedOpacity;
  discardQueuedOpacity();
  if (preview) useUi.getState().set({ layerOpacity: preview });
}

/** Document commands share one lane. Import status and cancellation bypass it. */
function enqueue<T>(work: () => Promise<T>): Promise<T> {
  const result = lane.then(work, work);
  lane = result.then(
    () => undefined,
    () => undefined,
  );
  return result;
}
function unwrap<T>(result: { status: 'ok'; data: T } | { status: 'error'; error: string }): T {
  if (result.status === 'error') throw new Error(result.error);
  return result.data;
}
function current() {
  return queryClient.getQueryData<EditorSnapshot>(queryKey) ?? empty;
}
function accept(snapshot: EditorSnapshot) {
  const previous = current().document;
  queryClient.setQueryData(queryKey, snapshot);
  const doc = snapshot.document;
  const ui = useUi.getState();
  if (previous?.sessionId !== doc?.sessionId) {
    discardQueuedOpacity();
    ui.set({
      inspectionTarget: doc ? { kind: 'animation', id: doc.animations[0].id } : null,
      animationId: doc?.animations[0].id ?? null,
      frameId: doc?.animations[0].frames[0].frameId ?? null,
      selectedFrames: doc ? [doc.animations[0].frames[0].frameId] : [],
      anchorFrame: doc?.animations[0].frames[0].frameId ?? null,
      playing: false,
      movePreview: null,
      stroke: null,
      selection: null,
      draft: null,
      deletion: null,
      newProjectOpen: false,
      importDialog: null,
      colorId: doc?.palette[0]?.id ?? null,
      layerId: doc
        ? (animationDocument(doc, doc.animations[0].id).layers.at(-1)?.id ?? null)
        : null,
      layerOpacity: null,
      palettePage: 0,
      externalPalettePage: 0,
      zoom: null,
      pan: { x: 0, y: 0 },
    });
  } else if (doc && !doc.palette.some((entry) => entry.id === ui.colorId)) {
    ui.set({ colorId: doc.palette[0]?.id ?? null, palettePage: 0 });
  }
  if (doc) {
    const state = useUi.getState();
    if (state.selection && !selectionMatches(state.selection, doc, state.layerId, state.frameId))
      state.set({ selection: null });
    const oldAnimationIndex =
      previous?.animations.findIndex((a) => a.id === state.animationId) ?? 0;
    const animation =
      doc.animations.find((a) => a.id === state.animationId) ??
      doc.animations[Math.min(Math.max(0, oldAnimationIndex), doc.animations.length - 1)];
    const view = animationDocument(doc, animation.id);
    const layerId = view.layers.some((layer) => layer.id === state.layerId)
      ? state.layerId
      : (view.layers.at(-1)?.id ?? null);
    const oldIndex =
      previous?.animations
        .find((a) => a.id === state.animationId)
        ?.frames.findIndex((f) => f.frameId === state.frameId) ?? 0;
    const frameId = view.frames.some((f) => f.id === state.frameId)
      ? state.frameId!
      : view.frames[Math.min(Math.max(0, oldIndex), view.frames.length - 1)].id;
    const selected =
      frameId !== state.frameId
        ? [frameId]
        : state.selectedFrames.filter((id) => view.frames.some((f) => f.id === id));
    state.set({
      animationId: animation.id,
      layerId,
      selection: selectionMatches(state.selection, view, layerId, frameId) ? state.selection : null,
      inspectionTarget: reconcileInspection(state.inspectionTarget, view),
      frameId,
      selectedFrames: selected.length ? selected : [frameId],
      anchorFrame: view.frames.some((f) => f.id === state.anchorFrame)
        ? state.anchorFrame
        : frameId,
    });
  }
  if (snapshot.warning) ui.set({ notice: snapshot.warning });
}
function guardFor(sessionId: string | undefined): DocumentGuard {
  const doc = current().document;
  if (!doc || !sessionId || doc.sessionId !== sessionId) {
    throw new Error('This action belongs to a project that is no longer open.');
  }
  return { sessionId: doc.sessionId, revision: doc.revision };
}
function message(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
export function registerStrokeFlusher(flusher: () => Promise<boolean>) {
  strokeFlusher = flusher;
  return () => {
    if (strokeFlusher === flusher) strokeFlusher = null;
  };
}
async function flushStroke() {
  return strokeFlusher ? strokeFlusher() : true;
}

type Job = () => Promise<void>;
function useController() {
  const query = useQuery({
    queryKey,
    enabled: desktop,
    queryFn: ({ signal }) =>
      enqueue(async () => {
        if (signal.aborted) throw new DOMException('Aborted', 'AbortError');
        const snapshot = unwrap(await commands.snapshot());
        if (signal.aborted) throw new DOMException('Aborted', 'AbortError');
        return snapshot;
      }),
  });
  const pending = useUi((state) => state.pending);
  const flowPending = useUi((state) => state.flowPending);
  const newProjectOpen = useUi((state) => state.newProjectOpen);
  const importOpen = useUi((state) => state.importDialog !== null);
  const mutation = useMutation({
    mutationKey: ['editor-command'],
    scope: { id: 'editor-session' },
    mutationFn: async (job: Job) => {
      // Cancel a slow startup/refetch before its response can overwrite a mutation.
      await queryClient.cancelQueries({ queryKey });
      await enqueue(job);
    },
  });
  const mutateAsync = mutation.mutateAsync;
  const actions = useMemo(() => {
    const run = async (
      job: Job,
      { background = false, finishStroke = false, fileFlow = false } = {},
    ): Promise<boolean> => {
      if (finishStroke || fileFlow) useUi.getState().set({ playing: false });
      // File flows own the command lane, except for the stroke they must flush.
      if (useUi.getState().flowPending && !fileFlow && !background) return false;
      if (!desktop) {
        useUi.getState().set({
          error:
            'Open Pixauro in the desktop app to access the Rust editor and local project files.',
        });
        return false;
      }
      // Document commands block new strokes internally, without dimming the UI.
      // A stroke commit has its own canvas guard and never enters global busy state.
      useUi.setState((state) => ({
        pending: state.pending + (background ? 0 : 1),
        error: null,
      }));
      try {
        // Flush outside the command lane: committing the stroke uses that same lane.
        if (finishStroke) {
          if (!(await flushStroke()) || !(await flushOpacity())) return false;
        }
        await mutateAsync(job);
        return true;
      } catch (error) {
        // A native flow may have saved the old document before a later open failed.
        // Refresh the authoritative snapshot without hiding the original failure.
        try {
          await enqueue(async () => accept(unwrap(await commands.snapshot())));
        } catch {
          /* keep the last known document */
        }
        useUi.getState().set({ error: message(error) });
        return false;
      } finally {
        if (!background) {
          useUi.setState((state) => ({ pending: Math.max(0, state.pending - 1) }));
        }
      }
    };
    const propertiesEdit = (
      id: string,
      patch: Partial<Pick<LayerSnapshot, 'name' | 'visible' | 'opacity' | 'locked'>>,
    ): LayerEdit => {
      const layer = current().document?.layers.find((entry) => entry.id === id);
      if (!layer) throw new Error('This layer no longer exists.');
      return {
        kind: 'properties',
        id,
        name: layer.name,
        visible: layer.visible,
        opacity: layer.opacity,
        locked: layer.locked,
        ...patch,
      };
    };
    // File flows and document actions share this flush. Concurrent blur/pointerup
    // events reuse the same promise, so a slider gesture creates one history entry.
    const flushOpacity = (): Promise<boolean> => {
      if (opacityCommit) return opacityCommit;
      publishQueuedOpacity();
      const preview = useUi.getState().layerOpacity;
      if (!preview) return Promise.resolve(true);
      const doc = current().document;
      if (
        doc?.sessionId === preview.sessionId &&
        doc.revision === preview.revision &&
        doc.layers.find((layer) => layer.id === preview.layerId)?.opacity === preview.opacity
      ) {
        useUi.getState().set({ layerOpacity: null });
        return Promise.resolve(true);
      }
      opacityCommit = run(
        async () => {
          const guard = guardFor(preview.sessionId);
          if (guard.revision !== preview.revision)
            throw new Error('The document changed while adjusting opacity. Please try again.');
          accept(
            unwrap(
              await commands.editLayer(
                guard,
                propertiesEdit(preview.layerId, { opacity: preview.opacity }),
              ),
            ),
          );
        },
        { fileFlow: true },
      ).finally(() => {
        if (useUi.getState().layerOpacity === preview) useUi.getState().set({ layerOpacity: null });
        opacityCommit = null;
      });
      return opacityCommit;
    };
    const projectFlow = (job: Job): Promise<boolean> => {
      if (flow) return flow;
      if (useUi.getState().importDialog) return Promise.resolve(false);
      // Keep unapplied dialogs intact when a native flow is canceled or fails.
      // accept() clears them only when a different document session is accepted.
      useUi.getState().set({ flowPending: true, playing: false });
      flow = (async () => {
        if (!(await flushStroke()) || !(await flushOpacity())) return false;
        return run(job, { fileFlow: true });
      })().finally(() => {
        flow = null;
        useUi.getState().set({ flowPending: false });
      });
      return flow;
    };
    const documentAction = (job: (guard: DocumentGuard) => Promise<EditorSnapshot>) => {
      const sessionId = current().document?.sessionId;
      return run(async () => accept(await job(guardFor(sessionId))), { finishStroke: true });
    };
    return {
      getDocument: () => current().document,
      newProject: () => {
        const ui = useUi.getState();
        if (flow || ui.draft || ui.deletion || ui.importDialog) return;
        ui.set({ newProjectOpen: true, error: null, playing: false });
      },
      createProject: (options: NewProjectOptions) =>
        projectFlow(async () => accept(unwrap(await commands.newProject(options)))),
      openProject: (path: string | null = null, expectedId: string | null = null) =>
        projectFlow(async () => accept(unwrap(await commands.openProject(path, expectedId)))),
      home: () => projectFlow(async () => accept(unwrap(await commands.closeProject()))),
      closeWindow: (): Promise<boolean> => {
        if (closing) return closing;
        // OS close is a distinct intent: an existing save/open must finish first.
        closing = (async () => {
          const importing = runningImport;
          if (importing) {
            await cancelPngImport();
            await importing.promise;
          }
          useUi.getState().set({ importDialog: null });
          if (flow) await flow;
          return projectFlow(async () => {
            unwrap(await commands.closeWindow());
          });
        })().finally(() => {
          closing = null;
        });
        return closing;
      },
      save: (saveAs = false) =>
        projectFlow(async () => accept(unwrap(await commands.saveProject(saveAs)))),
      forgetRecent: (id: string, path: string) =>
        run(async () => accept(unwrap(await commands.forgetRecent(id, path)))),
      projectPreview: (id: string, path: string, signal: AbortSignal) =>
        enqueue(async () => {
          if (signal.aborted) throw new DOMException('Aborted', 'AbortError');
          return unwrap(await commands.projectPreview(id, path));
        }),
      duplicateRecent: (id: string, path: string) =>
        projectFlow(async () => accept(unwrap(await commands.duplicateRecent(id, path)))),
      revealRecent: (id: string, path: string) =>
        run(async () => {
          unwrap(await commands.revealRecent(id, path));
        }),
      openPngImport: (path: string) => {
        const ui = useUi.getState();
        const doc = current().document;
        if (!doc || ui.importDialog || ui.draft || ui.deletion || ui.newProjectOpen || flow)
          return Promise.resolve(false);
        const { animationId, frameId, layerId } = ui;
        if (!frameId || !layerId) return Promise.resolve(false);
        return run(
          async () => {
            const guard = guardFor(doc.sessionId);
            useUi.getState().set({
              importDialog: {
                path,
                ...guard,
                animationId,
                frameId,
                layerId,
                canvasWidth: doc.width,
                canvasHeight: doc.height,
              },
              importMessage: null,
              importProgress: null,
            });
          },
          { finishStroke: true },
        );
      },
      cancelPngImport,
      prepareSpriteImport: async (
        options: PngImportOptions,
        slices: SpriteSlice[],
        removeSourceBackground: boolean,
        sourceBackgroundEdge: number,
      ): Promise<SpritePreview | null> => {
        const dialog = useUi.getState().importDialog;
        if (!dialog || runningImport) return null;
        const job: RunningImport = { id: crypto.randomUUID(), cancelled: false, promise: null };
        runningImport = job;
        useUi.getState().set({
          importRunning: true,
          importCancelling: false,
          importProgress: null,
          importMessage: null,
        });
        let preview: SpritePreview | null = null;
        job.promise = run(async () => {
          if (job.cancelled) return;
          preview = unwrap(
            await commands.prepareSpritePreview({
              path: dialog.path,
              removeSourceBackground,
              sourceBackgroundEdge,
              options,
              slices,
              jobId: job.id,
              canvasWidth: dialog.canvasWidth,
              canvasHeight: dialog.canvasHeight,
            }),
          );
          if (job.cancelled) preview = null;
          if (!preview)
            useUi.getState().set({ importMessage: 'Processing cancelled. Your slices are kept.' });
        }).finally(() => {
          if (runningImport === job) runningImport = null;
          useUi
            .getState()
            .set({ importRunning: false, importCancelling: false, importProgress: null });
        });
        void pollPngImport(job);
        await job.promise;
        return preview;
      },
      commitSpriteImport: (
        name: string,
        preview: SpritePreview,
        assembly: ImportAssembly,
        currentFrame: boolean,
      ) => {
        const dialog = useUi.getState().importDialog;
        if (!dialog || runningImport) return Promise.resolve(false);
        return run(async () => {
          const guard = guardFor(dialog.sessionId);
          if (guard.revision !== dialog.revision)
            throw new Error('The project changed. Close the importer and drop the PNG again.');
          const previous = new Set(current().document!.animations.map((a) => a.id));
          const result = unwrap(
            await commands.commitSpriteImport({
              guard,
              above: dialog.layerId,
              currentFrame: currentFrame ? dialog.frameId : null,
              name,
              preview,
              assembly,
            }),
          );
          accept(result.snapshot);
          const first = result.snapshot.document?.animations.find((a) => !previous.has(a.id));
          const frameId = first?.frames[0].frameId ?? dialog.frameId;
          useUi.getState().set({
            importDialog: null,
            layerId: result.layerId,
            animationId: first?.id ?? dialog.animationId,
            frameId,
            selectedFrames: [frameId],
            anchorFrame: frameId,
            inspectionTarget: { kind: 'layer', id: result.layerId },
            notice: `Imported ${preview.frames.length} frames. Undo removes the entire import.`,
          });
        });
      },
      historyJump: (position: number) =>
        documentAction(async (guard) => unwrap(await commands.historyJump(guard, position))),
      stepHistory: (direction: -1 | 1) =>
        documentAction(async (guard) => {
          // Relative undo/redo must use the position AFTER the active stroke commits.
          const snapshot = current();
          const doc = snapshot.document!;
          if (direction < 0 ? !doc.canUndo : !doc.canRedo) return snapshot;
          return unwrap(await commands.historyJump(guard, doc.history.position + direction));
        }),
      withSettledDocument: (action: (doc: DocumentSnapshot) => void) => {
        const sessionId = current().document?.sessionId;
        return run(
          async () => {
            guardFor(sessionId);
            action(current().document!);
          },
          { finishStroke: true },
        );
      },
      pickColor: (x: number, y: number) => {
        const doc = current().document;
        const ui = useUi.getState();
        if (!doc) return Promise.resolve(false);
        const view = animationDocument(doc, ui.animationId);
        if (x < 0 || y < 0 || x >= view.width || y >= view.height) return Promise.resolve(false);
        const frame = ui.frameId ?? view.frames[0].id;
        const color = pickFrameColor(view, frame, x, y);
        const quickPick = ui.tool === 'brush' || ui.tool === 'eraser';
        if (!color && !quickPick) return Promise.resolve(false);
        return run(async () => {
          const guard = guardFor(doc.sessionId);
          if (guard.revision !== doc.revision)
            throw new Error('The document changed while picking a color. Please try again.');
          if (!color) {
            useUi.getState().set({ tool: 'eraser' });
            return;
          }
          let entry = doc.palette.find(
            (entry) =>
              entry.color.red === color.red &&
              entry.color.green === color.green &&
              entry.color.blue === color.blue &&
              entry.color.alpha === color.alpha,
          );
          let palette = doc.palette;
          if (!entry) {
            const result = unwrap(await commands.addPaletteColor(guard, color));
            accept(result);
            palette = result.document!.palette;
            entry = palette.at(-1)!;
          }
          const entries = palette.filter((item) => item.source === entry.source);
          useUi.getState().set({
            colorId: entry.id,
            ...(quickPick ? { tool: 'brush' as const } : {}),
            [entry.source === 'custom' ? 'palettePage' : 'externalPalettePage']: Math.floor(
              entries.findIndex((item) => item.id === entry.id) / 16,
            ),
          });
        });
      },
      addColor: (color: RGBA) =>
        documentAction(async (guard) => {
          const result = unwrap(await commands.addPaletteColor(guard, color));
          const last = result.document?.palette.at(-1);
          if (last)
            useUi.getState().set({
              colorId: last.id,
              palettePage: Math.floor(
                ((result.document?.palette.filter((entry) => entry.source === 'custom').length ??
                  1) -
                  1) /
                  16,
              ),
            });
          return result;
        }),
      editColor: (id: string, color: RGBA) =>
        documentAction(async (guard) => unwrap(await commands.setPaletteColor(guard, id, color))),
      prunePalette: () =>
        documentAction(async (guard) => {
          const before = current().document!.palette.length;
          const result = unwrap(await commands.prunePalette(guard));
          const removed = before - result.document!.palette.length;
          useUi.getState().set({
            palettePage: 0,
            externalPalettePage: 0,
            notice: removed
              ? `Removed ${removed} unused colors. Undo restores them.`
              : 'No unused colors to remove. At least one palette color is kept.',
          });
          return result;
        }),
      removeColor: (id: string, replacement: string | null) =>
        documentAction(async (guard) => {
          const result = unwrap(await commands.removePaletteColor(guard, id, replacement));
          const entry = result.document?.palette.find((entry) => entry.id === replacement);
          if (entry) {
            const entries = result.document!.palette.filter(
              (color) => color.source === entry.source,
            );
            useUi.getState().set({
              colorId: entry.id,
              [entry.source === 'custom' ? 'palettePage' : 'externalPalettePage']: Math.floor(
                entries.findIndex((color) => color.id === entry.id) / 16,
              ),
            });
          }
          return result;
        }),
      previewLayerOpacity: (id: string, opacity: number) => {
        const ui = useUi.getState();
        const doc = current().document;
        if (
          !doc ||
          ui.pending > 0 ||
          ui.flowPending ||
          ui.stroke ||
          ui.draft ||
          ui.deletion ||
          ui.newProjectOpen
        )
          return;
        if (!doc.layers.some((layer) => layer.id === id)) return;
        const previous = queuedOpacity ?? ui.layerOpacity;
        if (
          previous?.sessionId === doc.sessionId &&
          previous.revision === doc.revision &&
          previous.layerId === id &&
          previous.opacity === opacity
        )
          return;
        queuedOpacity = { sessionId: doc.sessionId, revision: doc.revision, layerId: id, opacity };
        if (opacityFrame === null) opacityFrame = requestAnimationFrame(publishQueuedOpacity);
      },
      commitLayerOpacity: flushOpacity,
      cancelLayerOpacity: () => {
        if (!opacityCommit) {
          discardQueuedOpacity();
          useUi.getState().set({ layerOpacity: null });
        }
      },
      updateLayer: (
        id: string,
        patch: Partial<Pick<LayerSnapshot, 'name' | 'visible' | 'opacity' | 'locked'>>,
      ) =>
        documentAction(async (guard) =>
          unwrap(await commands.editLayer(guard, propertiesEdit(id, patch))),
        ),
      selectAnimation: (id: string) => {
        const sessionId = current().document?.sessionId;
        return run(
          async () => {
            guardFor(sessionId);
            const animation = current().document?.animations.find((a) => a.id === id);
            if (!animation) throw new Error('This animation no longer exists.');
            const state = useUi.getState();
            const layers = animationDocument(current().document!, id).layers;
            const frameId = animation.frames.some((f) => f.frameId === state.frameId)
              ? state.frameId!
              : animation.frames[0].frameId;
            state.set({
              animationId: id,
              layerId: layers.find((l) => l.id === state.layerId)?.id ?? layers.at(-1)?.id ?? null,
              selection: null,
              frameId,
              selectedFrames: [frameId],
              anchorFrame: frameId,
              inspectionTarget: { kind: 'animation', id },
              playing: false,
            });
          },
          { finishStroke: true },
        );
      },
      selectLayer: (id: string) => {
        const sessionId = current().document?.sessionId;
        return run(
          async () => {
            guardFor(sessionId);
            const doc = current().document;
            if (
              !doc ||
              !animationDocument(doc, useUi.getState().animationId).layers.some(
                (layer) => layer.id === id,
              )
            )
              throw new Error('This layer no longer exists.');
            useUi.getState().set({
              layerId: id,
              inspectionTarget: { kind: 'layer', id },
              selectedFrames: useUi.getState().frameId ? [useUi.getState().frameId!] : [],
              anchorFrame: useUi.getState().frameId,
            });
          },
          { finishStroke: true },
        );
      },
      editLayer: (edit: LayerEdit) =>
        documentAction(async (guard) => {
          const doc = current().document!;
          const layers = animationDocument(doc, useUi.getState().animationId).layers;
          const before = new Set(doc.layers.map((layer) => layer.id));
          const removedPosition =
            edit.kind === 'remove' || edit.kind === 'mergeDown'
              ? layers.findIndex((layer) => layer.id === edit.id)
              : -1;
          const neighbor =
            removedPosition >= 0
              ? (layers[removedPosition - 1] ?? layers[removedPosition + 1])?.id
              : undefined;
          const result = unwrap(await commands.editLayer(guard, edit));
          const added = result.document?.layers.find((layer) => !before.has(layer.id));
          if (added)
            useUi
              .getState()
              .set({ layerId: added.id, inspectionTarget: { kind: 'layer', id: added.id } });
          else if (neighbor && result.document?.layers.some((layer) => layer.id === neighbor))
            useUi.getState().set({ layerId: neighbor });
          return result;
        }),
      editAnimation: (edit: AnimationEdit) =>
        documentAction(async (guard) => {
          const beforeAnimations = new Set(current().document?.animations.map((a) => a.id));
          const before = new Set(current().document?.frames.map((f) => f.id));
          const result = unwrap(await commands.editAnimation(guard, edit));
          const animation = result.document?.animations.find((a) => !beforeAnimations.has(a.id));
          const added = result.document?.frames.find((f) => !before.has(f.id));
          if (animation) {
            const frameId = animation.frames[0].frameId;
            useUi.getState().set({
              animationId: animation.id,
              frameId,
              selectedFrames: [frameId],
              anchorFrame: frameId,
              inspectionTarget: { kind: 'animation', id: animation.id },
            });
          } else if (added) {
            const owner = result.document!.animations.find((a) =>
              a.frames.some((f) => f.frameId === added.id),
            )!;
            useUi.getState().set({
              animationId: owner.id,
              frameId: added.id,
              selectedFrames: [added.id],
              anchorFrame: added.id,
              inspectionTarget: { kind: 'frames', ids: [added.id] },
            });
          }
          return result;
        }),
      updateCelOffset: (layerId: string, frameId: string, axis: 'x' | 'y', value: number) =>
        documentAction(async (guard) => {
          const cel = celAt(
            current().document?.layers.find((l) => l.id === layerId),
            frameId,
          );
          if (!cel) throw new Error('This cel no longer exists.');
          return unwrap(
            await commands.editAnimation(guard, {
              kind: 'offset',
              layer: layerId,
              frame: frameId,
              x: axis === 'x' ? value : cel.x,
              y: axis === 'y' ? value : cel.y,
            }),
          );
        }),
      commitCelMove: (move: CelMove) =>
        run(
          async () => {
            guardFor(move.sessionId);
            accept(
              unwrap(
                await commands.editAnimation(
                  { sessionId: move.sessionId, revision: move.revision },
                  {
                    kind: 'offset',
                    layer: move.layerId,
                    frame: move.frameId,
                    x: move.x,
                    y: move.y,
                  },
                ),
              ),
            );
          },
          { background: true },
        ),
      commitStroke: (stroke: Stroke) =>
        run(
          async () => {
            guardFor(stroke.sessionId);
            // Do not refresh a stroke's revision: it previews the snapshot at pointerdown.
            accept(
              unwrap(
                await commands.paintStroke(
                  { sessionId: stroke.sessionId, revision: stroke.revision },
                  stroke.layerId,
                  stroke.frameId,
                  stroke.colorId,
                  stroke.coverage(),
                ),
              ),
            );
          },
          { background: true },
        ),
      deleteSelection: () => {
        const ui = useUi.getState();
        const selection = ui.selection;
        if (
          !selection ||
          ui.pending > 0 ||
          ui.flowPending ||
          ui.playing ||
          ui.draft ||
          ui.deletion ||
          ui.newProjectOpen ||
          ui.importDialog
        )
          return Promise.resolve(false);
        return documentAction(async (guard) => {
          const doc = current().document!;
          const state = useUi.getState();
          if (
            state.selection !== selection ||
            !selectionMatches(selection, doc, state.layerId, state.frameId)
          )
            return current();
          const layer = doc.layers.find((entry) => entry.id === selection.layerId);
          if (!layer || layer.locked || !layer.visible || layer.opacity === 0) {
            state.set({ notice: 'Select a visible, unlocked layer to delete selected pixels.' });
            return current();
          }
          return unwrap(
            await commands.paintStroke(
              guard,
              selection.layerId,
              selection.frameId,
              null,
              Array.from(selection.bitmap),
            ),
          );
        });
      },
      exportPng: (palette = false) =>
        projectFlow(async () => {
          const result = unwrap(
            await commands.exportPng(
              guardFor(current().document?.sessionId),
              palette,
              useUi.getState().frameId ?? current().document!.frames[0].id,
            ),
          );
          if (result)
            useUi
              .getState()
              .set({ notice: `Exported ${palette ? 'palette' : 'artwork'} to ${result}` });
        }),
      exportSpriteSheet: (allAnimations = false) =>
        projectFlow(async () => {
          const doc = current().document!;
          const animation = selectedAnimation(doc, useUi.getState().animationId);
          const result = unwrap(
            await commands.exportSpriteSheet(
              guardFor(doc.sessionId),
              allAnimations ? null : animation.id,
            ),
          );
          if (result)
            useUi.getState().set({
              notice: `Exported ${allAnimations ? 'all animations' : animation.name} as PNG + JSON: ${result}`,
            });
        }),
    };
  }, [mutateAsync]);
  const snapshot = query.data ?? empty;
  useEffect(() => {
    const doc = current().document;
    const state = useUi.getState();
    if (doc) {
      const animation = selectedAnimation(doc, state.animationId);
      if (
        state.animationId !== animation.id ||
        !animation.frames.some((f) => f.frameId === state.frameId)
      ) {
        const frameId = animation.frames[0].frameId;
        state.set({
          animationId: animation.id,
          frameId,
          selectedFrames: [frameId],
          anchorFrame: frameId,
          playing: false,
        });
      }
    }
    if (doc) {
      const layers = animationDocument(doc, useUi.getState().animationId).layers;
      if (!layers.some((layer) => layer.id === state.layerId))
        state.set({ layerId: layers.at(-1)?.id ?? null });
    }
    if (doc && !doc.palette.some((entry) => entry.id === state.colorId)) {
      state.set({ colorId: doc.palette[0]?.id ?? null, palettePage: 0 });
    }
  }, [snapshot]);
  useEffect(() => {
    if (!desktop) return;
    let disposed = false;
    const cleanup: (() => void)[] = [];
    const remember = (unlisten: () => void) => {
      if (disposed) unlisten();
      else cleanup.push(unlisten);
    };
    void listen('editor-close-requested', () => {
      void actions.closeWindow();
    })
      .then(remember)
      .catch((error) => {
        useUi.getState().set({ error: `Could not register close protection: ${message(error)}` });
      });
    void getCurrentWebviewWindow()
      .onDragDropEvent(({ payload }) => {
        const ui = useUi.getState();
        if (payload.type === 'enter') {
          ui.set({
            draggingFile: true,
            draggingPng: payload.paths.length === 1 && /\.png$/i.test(payload.paths[0]),
          });
          return;
        }
        if (payload.type === 'over') return;
        ui.set({ draggingFile: false, draggingPng: false });
        if (payload.type !== 'drop') return;
        if (flow || ui.newProjectOpen || ui.draft || ui.deletion || ui.importDialog) {
          ui.set({ error: 'Finish the current dialog or file operation before dropping a file.' });
          return;
        }
        if (payload.paths.length !== 1) {
          ui.set({ error: 'Drop one PNG image or one Pixauro project JSON file at a time.' });
          return;
        }
        const path = payload.paths[0];
        if (/\.png$/i.test(path)) {
          if (!current().document) {
            ui.set({ error: 'Create or open a project before importing a PNG image.' });
            return;
          }
          const bounds = window.document
            .querySelector('[data-png-drop-target]')
            ?.getBoundingClientRect();
          // Tauri reports physical pixels; DOM bounds use CSS pixels, including on Retina screens.
          const x = payload.position.x / window.devicePixelRatio;
          const y = payload.position.y / window.devicePixelRatio;
          if (
            !bounds ||
            x < bounds.left ||
            x >= bounds.right ||
            y < bounds.top ||
            y >= bounds.bottom
          ) {
            ui.set({ error: 'Drop the PNG image onto the canvas editing area.' });
            return;
          }
          void actions.openPngImport(path);
          return;
        }
        if (!/\.json$/i.test(path)) {
          ui.set({ error: 'Drop one PNG image or one Pixauro project JSON file.' });
          return;
        }
        void actions.openProject(path);
      })
      .then(remember)
      .catch((error) => useUi.getState().set({ error: message(error) }));
    return () => {
      disposed = true;
      cleanup.forEach((fn) => fn());
    };
  }, [actions]);
  return {
    ...actions,
    snapshot,
    document: snapshot.document,
    recent: snapshot.recent,
    // Interaction guards use busy; visual disabling only uses initial loading.
    busy: flowPending || (desktop && query.isLoading),
    drawingBlocked:
      pending > 0 || flowPending || newProjectOpen || importOpen || (desktop && query.isLoading),
    desktop,
    loading: desktop && query.isLoading,
    loadError: query.error ? message(query.error) : null,
    reload: () => {
      void query.refetch();
    },
  };
}
const Context = createContext<ReturnType<typeof useController> | null>(null);
export function EditorProvider({ children }: { children: ReactNode }) {
  const value = useController();
  return <Context.Provider value={value}>{children}</Context.Provider>;
}
export function useEditor() {
  const value = useContext(Context);
  if (!value) throw new Error('EditorProvider is missing');
  return value;
}
