import { useEditor } from '../editorController';
import { useUi } from '../editorUiState';
import type { DocumentSnapshot, LayerSnapshot } from '../generated/bindings';
import { celAt, linkedFrames, frameNumbers } from '../animation';
import { Button } from './ui/button';
import { InspectorFields } from './InspectorFields';

export function CelInspector({
  doc,
  layer,
  frameIds,
  disabled,
}: {
  doc: DocumentSnapshot;
  layer: LayerSnapshot;
  frameIds: string[];
  disabled: boolean;
}) {
  const editor = useEditor();
  const ui = useUi();
  const selected = doc.frames.filter((f) => frameIds.includes(f.id));
  const frame = selected[0];
  if (!frame) return null;
  const index = doc.frames.indexOf(frame);
  const cel = celAt(layer, frame.id);
  const group = linkedFrames(doc, layer, frame.id);
  const anchor = selected.find((f) => celAt(layer, f.id));
  const locked = disabled || layer.locked;
  const jump = (id: string) =>
    void editor.withSettledDocument(() =>
      ui.set({
        layerId: layer.id,
        frameId: id,
        selectedFrames: [id],
        anchorFrame: id,
        inspectionTarget: { kind: 'cels', layerId: layer.id, frameIds: [id] },
      }),
    );
  const previous = celAt(layer, doc.frames[index - 1]?.id ?? null);
  return (
    <div className="property-section">
      <div className="property-heading">
        <h2>Cel</h2>
        <span>
          {layer.name} · frame {index + 1}
        </span>
      </div>
      {selected.length > 1 ? (
        <>
          <div>
            <p>
              {selected.length} cels selected · frames{' '}
              {frameNumbers(
                doc,
                selected.map((f) => f.id),
              )}
            </p>
            <p className="muted">
              {selected.filter((f) => celAt(layer, f.id)).length} with images ·{' '}
              {selected.filter((f) => linkedFrames(doc, layer, f.id).length > 1).length} shared
            </p>
          </div>
          <div className="property-actions">
            <Button
              variant="secondary"
              disabled={locked || !anchor}
              onClick={() =>
                void editor.editAnimation({
                  kind: 'link',
                  layer: layer.id,
                  frames: selected.map((f) => f.id),
                })
              }
            >
              Link
            </Button>
            <Button
              variant="secondary"
              disabled={locked || !selected.some((f) => linkedFrames(doc, layer, f.id).length > 1)}
              onClick={() =>
                void editor.editAnimation({
                  kind: 'unlink',
                  layer: layer.id,
                  frames: selected.map((f) => f.id),
                })
              }
            >
              Unlink
            </Button>
            <Button
              variant="secondary"
              disabled={locked || !anchor}
              onClick={() =>
                void editor.editAnimation({
                  kind: 'clear',
                  layer: layer.id,
                  frames: selected.map((f) => f.id),
                })
              }
            >
              Clear
            </Button>
          </div>
          {anchor && (
            <p className="muted">
              Link uses the image and offset of frame {doc.frames.indexOf(anchor) + 1} for every
              selected cel.
            </p>
          )}
        </>
      ) : (
        <>
          {cel ? (
            <>
              <div className="property-row property-image">
                <span className="property-label">Image</span>
                <div className="property-value">
                  <p>
                    {group.length > 1
                      ? `Shared by ${group.length} cels`
                      : `Unique image · ${layer.images.find((i) => i.id === cel.imageId)?.pixels.length ?? 0} pixels`}
                  </p>
                  {group.length > 1 && (
                    <div className="property-frame-links">
                      {group.map((id) => (
                        <Button
                          key={id}
                          variant="secondary"
                          disabled={disabled}
                          aria-pressed={id === frame.id}
                          onClick={() => jump(id)}
                        >
                          {doc.frames.findIndex((f) => f.id === id) + 1}
                        </Button>
                      ))}
                    </div>
                  )}
                  <p className="muted">
                    {group.length > 1
                      ? 'Editing this shared image changes every frame above. Offset belongs to each cel.'
                      : 'This image is used by this cel only.'}
                  </p>
                </div>
              </div>
              <InspectorFields
                disabled={locked}
                fields={(['x', 'y'] as const).map((axis) => ({
                  kind: 'number',
                  key: axis,
                  label: `Offset ${axis.toUpperCase()}`,
                  value: cel[axis],
                  min: -8192,
                  max: 8192,
                  unit: 'px',
                  commit: (value: string) =>
                    editor.updateCelOffset(layer.id, frame.id, axis, Number(value)),
                }))}
              />
            </>
          ) : (
            <p className="muted">
              Empty cel. Drawing on this layer and frame creates a new image for this cel only.
            </p>
          )}
          {group.length > 1 ? (
            <div className="property-actions">
              <Button
                variant="secondary"
                disabled={locked}
                onClick={() =>
                  void editor.editAnimation({
                    kind: 'unlink',
                    layer: layer.id,
                    frames: [frame.id],
                  })
                }
              >
                Unlink this cel
              </Button>
              <Button
                variant="secondary"
                disabled={disabled}
                onClick={() =>
                  void editor.withSettledDocument(() =>
                    ui.set({
                      layerId: layer.id,
                      frameId: frame.id,
                      anchorFrame: frame.id,
                      selectedFrames: group,
                      inspectionTarget: { kind: 'cels', layerId: layer.id, frameIds: group },
                    }),
                  )
                }
              >
                Select linked
              </Button>
            </div>
          ) : previous && previous.imageId !== cel?.imageId ? (
            <div className="property-actions">
              <Button
                variant="secondary"
                disabled={locked}
                onClick={() =>
                  void editor.editAnimation({
                    kind: 'linkPrevious',
                    layer: layer.id,
                    frame: frame.id,
                  })
                }
              >
                Link to previous frame
              </Button>
            </div>
          ) : null}
        </>
      )}
    </div>
  );
}
