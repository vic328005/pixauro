import type { ComponentType } from 'react';
import { useEditor } from '../editorController';
import { animationNameError } from '../inputValidation';
import { useUi } from '../editorUiState';
import { reconcileInspection, type InspectionTarget } from '../inspection';
import type { DocumentSnapshot } from '../generated/bindings';
import { animationDocument, frameNumbers } from '../animation';
import { animationDuration } from '../animationTiming';
import { InspectorFields } from './InspectorFields';
import { CelInspector } from './CelInspector';
import { LayerOpacityControl } from './LayerOpacityControl';
import './layers-panel.css';
import './timeline.css';
import './inspector.css';

type InspectorProps = { doc: DocumentSnapshot; target: InspectionTarget; disabled: boolean };

function LayerInspector({ doc, target, disabled }: InspectorProps) {
  const editor = useEditor();
  const ui = useUi();
  if (target.kind !== 'layer') return null;
  const layer = doc.layers.find((l) => l.id === target.id);
  if (!layer) return null;
  return (
    <div className="property-section">
      <div className="property-heading">
        <h2>Layer</h2>
        <span>{layer.name}</span>
      </div>
      <InspectorFields
        disabled={disabled}
        fields={[
          {
            kind: 'text',
            key: 'name',
            label: 'Name',
            value: layer.name,
            commit: (name) => editor.updateLayer(layer.id, { name }),
          },
          {
            kind: 'toggle',
            key: 'visible',
            label: 'Visible',
            value: layer.visible,
            commit: (visible) => editor.updateLayer(layer.id, { visible }),
          },
          {
            kind: 'toggle',
            key: 'locked',
            label: 'Locked',
            value: layer.locked,
            commit: (locked) => editor.updateLayer(layer.id, { locked }),
          },
          {
            kind: 'select',
            key: 'newFrames',
            label: 'New frames',
            value: layer.autoLink ? 'linked' : 'empty',
            options: [
              { value: 'empty', label: 'Empty' },
              { value: 'linked', label: 'Linked' },
            ],
            commit: (value) =>
              editor.editAnimation({ kind: 'autoLink', layer: layer.id, on: value === 'linked' }),
          },
        ]}
      />
      <LayerOpacityControl
        key={`${doc.sessionId}:${layer.id}`}
        layer={layer}
        value={ui.layerOpacity?.layerId === layer.id ? ui.layerOpacity.opacity : layer.opacity}
        disabled={disabled}
        blocked={disabled || editor.busy || !!ui.stroke || !!ui.movePreview}
      />
      {layer.locked && (
        <p className="muted">Drawing is locked. Layer properties remain editable.</p>
      )}
    </div>
  );
}

function AnimationInspector({ doc, target, disabled }: InspectorProps) {
  const editor = useEditor();
  if (target.kind !== 'animation') return null;
  const animation = doc.animations.find((a) => a.id === target.id);
  if (!animation) return null;
  return (
    <div className="property-section">
      <div className="property-heading">
        <h2>Animation</h2>
        <span>{animation.name}</span>
      </div>
      <InspectorFields
        disabled={disabled}
        fields={[
          {
            kind: 'text',
            key: 'name',
            label: 'Name',
            value: animation.name,
            getError: (name) =>
              animationNameError(
                name,
                doc.animations.filter((a) => a.id !== animation.id).map((a) => a.name),
              ),
            commit: (name) =>
              editor.editAnimation({ kind: 'renameAnimation', id: animation.id, name }),
          },
          {
            kind: 'readonly',
            key: 'frames',
            label: 'Frames',
            value: String(animation.frames.length),
          },
          {
            kind: 'readonly',
            key: 'fps',
            label: 'FPS',
            value: String(animation.fps),
          },
          {
            kind: 'readonly',
            key: 'duration',
            label: 'Duration',
            value: animationDuration(animation.frames.length, animation.fps),
          },
        ]}
      />
      <p className="muted">Edit this animation’s frame sequence and FPS in Timeline.</p>
    </div>
  );
}

function FramesInspector({ doc, target }: InspectorProps) {
  if (target.kind !== 'frames') return null;
  const frames = doc.frames.filter((f) => target.ids.includes(f.id));
  if (!frames.length) return null;
  return (
    <div className="property-section">
      <div className="property-heading">
        <h2>{frames.length > 1 ? `${frames.length} frames` : 'Frame'}</h2>
        <span>{frameNumbers(doc, target.ids)}</span>
      </div>
      <p className="muted">
        Frames contain artwork. FPS belongs to the animation and is edited in Timeline.
      </p>
    </div>
  );
}

function CelsInspector({ doc, target, disabled }: InspectorProps) {
  if (target.kind !== 'cels') return null;
  const layer = doc.layers.find((l) => l.id === target.layerId);
  return layer ? (
    <CelInspector doc={doc} layer={layer} frameIds={target.frameIds} disabled={disabled} />
  ) : null;
}

/** Register object editors here; ordinary properties use the shared field renderer. */
const inspectors: Record<InspectionTarget['kind'], ComponentType<InspectorProps>> = {
  animation: AnimationInspector,
  layer: LayerInspector,
  frames: FramesInspector,
  cels: CelsInspector,
};

export function Inspector() {
  const editor = useEditor();
  const ui = useUi();
  const doc = editor.document ? animationDocument(editor.document, ui.animationId) : null;
  if (!doc) return null;
  const target = reconcileInspection(ui.inspectionTarget, doc);
  const Content = target ? inspectors[target.kind] : null;
  const disabled = editor.drawingBlocked || !!ui.draft || !!ui.deletion || ui.playing;
  return (
    <section className="inspector-panel" aria-label="Inspector">
      {target && Content ? (
        <Content
          key={`${doc.sessionId}:${JSON.stringify(target)}`}
          doc={doc}
          target={target}
          disabled={disabled}
        />
      ) : (
        <p className="inspector-empty muted">
          Select an animation, layer, frame or cel to inspect it.
        </p>
      )}
    </section>
  );
}
