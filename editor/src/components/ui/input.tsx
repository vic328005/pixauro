import * as React from 'react';
import { cn } from 'cn';

function Input({ className, type, ...props }: React.ComponentProps<'input'>) {
  return (
    <input
      type={type}
      data-slot="input"
      className={cn('input', className)}
      {...props}
      autoCorrect="off"
      autoCapitalize="none"
      spellCheck={false}
    />
  );
}

export { Input };
