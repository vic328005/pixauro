import { celAt, linkedFrames } from '../animation';
import { useUi } from '../editorUiState';
import type { AnimationEdit, DocumentSnapshot } from '../generated/bindings';
import { ContextMenu } from './ui/context-menu';

export type TimelineMenuTarget = {
  layer: string;
  frame: string;
  frames: string[];
  header: boolean;
};

export function TimelineContextMenu({
  doc,
  menu,
  edit,
  close,
}: {
  doc: DocumentSnapshot;
  menu: TimelineMenuTarget;
  edit: (change: AnimationEdit) => void;
  close: () => void;
}) {
  const ui = useUi();
  const menuLayer = doc.layers.find((l) => l.id === menu?.layer);
  const menuGroup = linkedFrames(doc, menuLayer, menu?.frame ?? null);
  const menuIndex = doc.frames.findIndex((f) => f.id === menu?.frame);
  const previous = celAt(menuLayer, doc.frames[menuIndex - 1]?.id ?? null);
  const current = celAt(menuLayer, menu?.frame ?? null);
  const menuSelected = doc.frames.filter((f) => menu?.frames.includes(f.id));
  const anchor = menuSelected.find((f) => celAt(menuLayer, f.id));
  return (
    <ContextMenu.Content
      aria-label="Timeline actions"
      onContextMenu={(event) => {
        event.preventDefault();
        close();
      }}
    >
      {menu.header ? (
        <>
          <ContextMenu.Item onSelect={() => edit({ kind: 'newFrame', after: menu.frame })}>
            New frame after {menuIndex + 1}
          </ContextMenu.Item>
          <ContextMenu.Item
            onSelect={() => edit({ kind: 'duplicateFrame', frame: menu.frame, linked: false })}
          >
            Duplicate · copied images
          </ContextMenu.Item>
          <ContextMenu.Item
            onSelect={() => edit({ kind: 'duplicateFrame', frame: menu.frame, linked: true })}
          >
            Duplicate · linked cels
          </ContextMenu.Item>
          <ContextMenu.Separator />
          <ContextMenu.Item
            disabled={
              doc.frames.length === 1 || doc.layers.some((l) => l.locked && !!celAt(l, menu.frame))
            }
            onSelect={() => edit({ kind: 'deleteFrame', frame: menu.frame })}
          >
            Delete frame {menuIndex + 1}
          </ContextMenu.Item>
        </>
      ) : (
        <>
          {menu.frames.length === 1 ? (
            <ContextMenu.Item
              disabled={menuLayer?.locked || !previous || previous.imageId === current?.imageId}
              onSelect={() => edit({ kind: 'linkPrevious', layer: menu.layer, frame: menu.frame })}
            >
              Link to previous frame
            </ContextMenu.Item>
          ) : (
            <ContextMenu.Item
              disabled={menuLayer?.locked || !anchor}
              onSelect={() => edit({ kind: 'link', layer: menu.layer, frames: menu.frames })}
            >
              Link {menu.frames.length} cels to frame{' '}
              {anchor ? doc.frames.indexOf(anchor) + 1 : '—'}
            </ContextMenu.Item>
          )}
          <ContextMenu.Item
            disabled={
              menuLayer?.locked ||
              !menu.frames.some((f) => linkedFrames(doc, menuLayer, f).length > 1)
            }
            onSelect={() => edit({ kind: 'unlink', layer: menu.layer, frames: menu.frames })}
          >
            Unlink {menu.frames.length === 1 ? 'this cel' : `${menu.frames.length} cels`}
          </ContextMenu.Item>
          <ContextMenu.Item
            disabled={menuGroup.length < 2}
            onSelect={() => {
              ui.set({
                selectedFrames: menuGroup,
                inspectionTarget: { kind: 'cels', layerId: menu.layer, frameIds: menuGroup },
              });
              close();
            }}
          >
            Select {menuGroup.length} linked cels
          </ContextMenu.Item>
          <ContextMenu.Separator />
          <ContextMenu.Item
            disabled={menuLayer?.locked || !menu.frames.some((f) => celAt(menuLayer, f))}
            onSelect={() => edit({ kind: 'clear', layer: menu.layer, frames: menu.frames })}
          >
            Clear {menu.frames.length} cel{menu.frames.length === 1 ? '' : 's'}
          </ContextMenu.Item>
        </>
      )}
      <ContextMenu.Separator />
      <ContextMenu.Item
        onSelect={() => {
          ui.set({ celView: ui.celView === 'dots' ? 'thumbnails' : 'dots' });
          close();
        }}
      >
        {ui.celView === 'dots' ? 'Show cel thumbnails' : 'Show cel markers'}
      </ContextMenu.Item>
    </ContextMenu.Content>
  );
}
