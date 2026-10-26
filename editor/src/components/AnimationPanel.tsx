import { Plus, Trash2 } from 'lucide-react';
import { useEditor } from '../editorController';
import { animationNameError } from '../inputValidation';
import { selectedAnimation } from '../animation';
import { animationDuration } from '../animationTiming';
import { useUi } from '../editorUiState';
import { Button } from './ui/button';
import { CommitInput } from './ui/commit-input';
import { useAnimationReorder } from './useAnimationReorder';
import './animation-panel.css';

export function AnimationPanel() {
  const editor = useEditor();
  const ui = useUi();
  const doc = editor.document;
  const disabled = editor.drawingBlocked || !!ui.draft || !!ui.deletion;
  const canReorder = !disabled && !editor.busy && !ui.playing && (doc?.animations.length ?? 0) > 1;
  const { listRef, dragged, drop, startDrag, blockClick } = useAnimationReorder(canReorder);
  if (!doc) return null;
  const animation = selectedAnimation(doc, ui.animationId);
  const locked = doc.layers.some((l) => l.animationId === animation.id && l.locked);
  const removeReason =
    doc.animations.length === 1
      ? 'Keep at least one animation'
      : locked
        ? 'Unlock this animation’s layers to delete it'
        : 'Delete animation, its layers and frames · can be undone';
  const add = () => {
    let n = 1;
    while (doc.animations.some((a) => a.name === `Animation ${n}`)) n++;
    void editor.editAnimation({ kind: 'addAnimation', name: `Animation ${n}` });
  };
  return (
    <section className="animation-panel" aria-label="Animation">
      <div className="animation-toolbar">
        <span>{doc.animations.length} animations</span>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label="New animation"
          title="New animation"
          disabled={
            disabled ||
            editor.busy ||
            doc.animations.length >= 256 ||
            doc.frames.length >= 4096 ||
            doc.layers.length >= 256
          }
          onClick={add}
        >
          <Plus aria-hidden="true" />
        </Button>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label="Delete animation"
          title={removeReason}
          disabled={disabled || editor.busy || doc.animations.length === 1 || locked}
          onClick={() => void editor.editAnimation({ kind: 'deleteAnimation', id: animation.id })}
        >
          <Trash2 aria-hidden="true" />
        </Button>
      </div>
      <div
        ref={listRef}
        className={`animation-list${dragged ? ' is-reordering' : ''}`}
        aria-label="Animations"
      >
        {doc.animations.map((a) => (
          <button
            key={a.id}
            className="animation-item"
            data-animation-id={a.id}
            data-reorderable={canReorder || undefined}
            data-dragged={dragged === a.id || undefined}
            data-drop={drop?.id === a.id ? drop.edge : undefined}
            aria-pressed={a.id === animation.id}
            title={canReorder ? 'Drag to reorder · Alt+↑/↓ to move' : undefined}
            disabled={disabled}
            onPointerDown={(event) => startDrag(event, a.id)}
            onClick={(event) => {
              if (blockClick() && event.detail !== 0) return;
              void editor.selectAnimation(a.id);
            }}
            onKeyDown={(event) => {
              if (!canReorder || !event.altKey || !['ArrowUp', 'ArrowDown'].includes(event.key))
                return;
              event.preventDefault();
              const position =
                doc.animations.findIndex((item) => item.id === a.id) +
                (event.key === 'ArrowUp' ? -1 : 1);
              if (position >= 0 && position < doc.animations.length)
                void editor.editAnimation({ kind: 'moveAnimation', id: a.id, position });
            }}
          >
            <span className="animation-name" title={a.name}>
              {a.name}
            </span>
            <span className="animation-summary">
              {a.frames.length} frames · {a.fps} FPS · {animationDuration(a.frames.length, a.fps)}
            </span>
          </button>
        ))}
      </div>
      <div className="animation-details">
        <label htmlFor="animation-name">Name</label>
        <CommitInput
          key={`${doc.sessionId}:${animation.id}`}
          id="animation-name"
          label="Animation name"
          value={animation.name}
          getError={(name) =>
            animationNameError(
              name,
              doc.animations.filter((a) => a.id !== animation.id).map((a) => a.name),
            )
          }
          disabled={disabled}
          onCommit={(name) =>
            editor.editAnimation({ kind: 'renameAnimation', id: animation.id, name })
          }
        />
      </div>
    </section>
  );
}
