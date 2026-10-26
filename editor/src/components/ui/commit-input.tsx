import { useEffect, useId, useRef, useState } from 'react';
import { Popover } from 'radix-ui';
import { nameError } from '../../inputValidation';

/** Enter blurs once; invalid commits restore the official value with inline feedback. */
export function CommitInput({
  label,
  id,
  value,
  onCommit,
  onFinish,
  autoFocus = false,
  disabled = false,
  min,
  max,
  integer = true,
  compact = false,
  className = '',
  placeholder,
  getError,
}: {
  className?: string;
  label: string;
  id?: string;
  value: string;
  onCommit: (value: string) => void | Promise<boolean>;
  onFinish?: () => void;
  autoFocus?: boolean;
  disabled?: boolean;
  min?: number;
  max?: number;
  integer?: boolean;
  compact?: boolean;
  placeholder?: string;
  getError?: (value: string) => string | null;
}) {
  const errorId = useId();
  const [draft, setDraft] = useState(value);
  const [pending, setPending] = useState(false);
  const [feedback, setFeedback] = useState<string | null>(null);
  const official = useRef(value);
  const cancel = useRef(false);
  useEffect(() => {
    official.current = value;
    setDraft(value);
    setFeedback(null);
  }, [value]);

  function validationError(text: string) {
    if (min === undefined) return nameError(text) ?? getError?.(text) ?? null;
    const number = Number(text);
    if (
      !text ||
      !Number.isFinite(number) ||
      (integer && !Number.isInteger(number)) ||
      number < min ||
      number > (max ?? Infinity)
    ) {
      return `Enter ${integer ? 'a whole number' : 'a number'} ${max === undefined ? `of at least ${min}` : `from ${min} to ${max}`}.`;
    }
    return getError?.(text) ?? null;
  }

  const field = (
    <span className="commit-field">
      <input
        className={`timeline-input ${className}`}
        id={id}
        aria-label={label}
        aria-invalid={!!feedback}
        aria-describedby={feedback ? errorId : undefined}
        value={draft}
        placeholder={placeholder}
        autoFocus={autoFocus}
        autoCorrect="off"
        autoCapitalize="none"
        spellCheck={false}
        disabled={disabled || pending}
        inputMode={min === undefined ? 'text' : integer ? 'numeric' : 'decimal'}
        onChange={(e) => {
          setDraft(e.target.value);
          setFeedback(validationError(e.target.value.trim()));
        }}
        onBlur={async () => {
          if (pending) return;
          if (cancel.current) {
            cancel.current = false;
            setDraft(official.current);
            setFeedback(null);
            onFinish?.();
            return;
          }
          const text = draft.trim();
          // An untouched mixed-value field has no value to commit.
          if (text === value && !feedback) {
            onFinish?.();
            return;
          }
          const error = validationError(text);
          if (error) {
            setDraft(official.current);
            setFeedback(`${error} Previous value kept.`);
            return;
          }
          setPending(true);
          try {
            const saved = text === value || (await onCommit(text)) !== false;
            setDraft(official.current);
            setFeedback(saved ? null : 'Could not save this value. Previous value kept.');
            if (saved) onFinish?.();
          } catch {
            setDraft(official.current);
            setFeedback('Could not save this value. Previous value kept.');
          } finally {
            setPending(false);
          }
        }}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault();
            e.currentTarget.blur();
          }
          if (e.key === 'Escape') {
            e.preventDefault();
            e.stopPropagation();
            cancel.current = true;
            e.currentTarget.blur();
          }
        }}
      />
      {!compact && feedback && (
        <span id={errorId} className="field-error" role="alert">
          {feedback}
        </span>
      )}
    </span>
  );
  if (!compact) return field;
  return (
    <Popover.Root open={!!feedback}>
      <Popover.Anchor asChild>{field}</Popover.Anchor>
      <Popover.Portal>
        <Popover.Content
          id={errorId}
          role="alert"
          className="commit-error-popover field-error"
          sideOffset={4}
          align="start"
          collisionPadding={8}
          onOpenAutoFocus={(event) => event.preventDefault()}
          onCloseAutoFocus={(event) => event.preventDefault()}
        >
          {feedback}
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
