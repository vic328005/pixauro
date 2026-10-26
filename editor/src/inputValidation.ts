/** Keep name rules aligned with the engine's trimmed, Unicode-character validation. */
export function nameError(value: string): string | null {
  const name = value.trim();
  if (!name) return 'Enter a name.';
  if (Array.from(name).length > 128) return 'Use no more than 128 characters.';
  if (
    Array.from(name).some((character) => {
      const code = character.codePointAt(0)!;
      return code < 32 || (code >= 127 && code <= 159);
    })
  )
    return 'Remove control characters from the name.';
  return null;
}

export function animationNameError(value: string, existingNames: readonly string[]): string | null {
  return (
    nameError(value) ??
    (existingNames.includes(value.trim()) ? 'An animation with this name already exists.' : null)
  );
}
