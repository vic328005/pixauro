import { create } from 'zustand';
import type { StrokePreview } from './brush';
import type { InspectionTarget } from './inspection';
import type { ImportProgress } from './generated/bindings';
import type { PixelSelection } from './selection';

/** UI-only state. Authoritative document, history and dirty status live in Rust. */
export type RGBA = { red: number; green: number; blue: number; alpha: number };
export type ColorDraft = {
  id: string | null;
  original: RGBA;
  color: RGBA;
  hex: string;
  invalidHex: boolean;
};
export type Stroke = StrokePreview;
/** Transient slider preview; committed once when the gesture finishes. */
export type LayerOpacityPreview = {
  sessionId: string;
  revision: string;
  layerId: string;
  opacity: number;
};
export type CelMove = {
  sessionId: string;
  revision: string;
  layerId: string;
  frameId: string;
  x: number;
  y: number;
};
export type PngImportDialog = {
  path: string;
  sessionId: string;
  revision: string;
  animationId: string | null;
  frameId: string;
  layerId: string;
  canvasWidth: number;
  canvasHeight: number;
};
interface UiState {
  inspectionTarget: InspectionTarget | null;
  tool: 'brush' | 'eraser' | 'pan' | 'move' | 'wand' | 'eyedropper';
  selection: PixelSelection | null;
  wandTolerance: number;
  wandContiguous: boolean;
  animationId: string | null;
  frameId: string | null;
  selectedFrames: string[];
  anchorFrame: string | null;
  playing: boolean;
  onion: boolean;
  celView: 'dots' | 'thumbnails';
  movePreview: CelMove | null;
  colorId: string | null;
  layerId: string | null;
  layerOpacity: LayerOpacityPreview | null;
  brushSize: number;
  palettePage: number;
  externalPalettePage: number;
  showGrid: boolean;
  backdrop: 'dark' | 'light';
  zoom: number | null;
  pan: { x: number; y: number };
  draft: ColorDraft | null;
  deletion: { id: string; replacement: string | null } | null;
  newProjectOpen: boolean;
  importDialog: PngImportDialog | null;
  importRunning: boolean;
  importCancelling: boolean;
  importProgress: ImportProgress | null;
  importMessage: string | null;
  stroke: Stroke | null;
  /** Internal document-command guard; never drives global disabled styling. */
  pending: number;
  /** File-operation interaction lock; preserve the current control appearance. */
  flowPending: boolean;
  error: string | null;
  notice: string | null;
  draggingFile: boolean;
  draggingPng: boolean;
  layoutReset: number;
  set: (value: Partial<Omit<UiState, 'set'>>) => void;
}
export const useUi = create<UiState>((set) => ({
  inspectionTarget: null,
  tool: 'brush',
  selection: null,
  wandTolerance: 0,
  wandContiguous: true,
  animationId: null,
  frameId: null,
  selectedFrames: [],
  anchorFrame: null,
  playing: false,
  onion: false,
  celView: 'dots',
  movePreview: null,
  colorId: null,
  layerId: null,
  layerOpacity: null,
  brushSize: 1,
  palettePage: 0,
  externalPalettePage: 0,
  showGrid: false,
  backdrop: 'dark',
  zoom: null,
  pan: { x: 0, y: 0 },
  draft: null,
  deletion: null,
  newProjectOpen: false,
  importDialog: null,
  importRunning: false,
  importCancelling: false,
  importProgress: null,
  importMessage: null,
  stroke: null,
  pending: 0,
  flowPending: false,
  error: null,
  notice: null,
  draggingFile: false,
  draggingPng: false,
  layoutReset: 0,
  set: (value) =>
    set((state) => {
      const changedTarget =
        (value.layerId !== undefined && value.layerId !== state.layerId) ||
        (value.frameId !== undefined && value.frameId !== state.frameId) ||
        (value.animationId !== undefined && value.animationId !== state.animationId) ||
        value.playing === true;
      return { ...value, ...(changedTarget ? { selection: null } : {}) };
    }),
}));
