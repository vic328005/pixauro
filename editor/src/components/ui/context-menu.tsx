import { useEffect, useState, type ComponentProps } from 'react';
import { cn } from 'cn';
import {
  ContextMenu as ContextMenuPrimitive,
  DropdownMenu as DropdownMenuPrimitive,
} from 'radix-ui';
import './menu.css';

function useMenuState({
  open,
  defaultOpen = false,
  onOpenChange,
}: {
  open?: boolean;
  defaultOpen?: boolean;
  onOpenChange?: (open: boolean) => void;
}) {
  const [internalOpen, setInternalOpen] = useState(defaultOpen);
  const isOpen = open ?? internalOpen;
  useEffect(() => {
    if (!isOpen) return;
    const close = () => {
      setInternalOpen(false);
      onOpenChange?.(false);
    };
    window.addEventListener('blur', close);
    window.addEventListener('resize', close);
    return () => {
      window.removeEventListener('blur', close);
      window.removeEventListener('resize', close);
    };
  }, [isOpen, onOpenChange]);
  return {
    open: isOpen,
    onOpenChange: (next: boolean) => {
      setInternalOpen(next);
      onOpenChange?.(next);
    },
  };
}

function ContextMenuRoot(props: ComponentProps<typeof ContextMenuPrimitive.Root>) {
  const state = useMenuState(props);
  return <ContextMenuPrimitive.Root {...props} {...state} />;
}

function ContextMenuContent({
  className,
  ...props
}: ComponentProps<typeof ContextMenuPrimitive.Content>) {
  return (
    <ContextMenuPrimitive.Portal>
      <ContextMenuPrimitive.Content
        collisionPadding={8}
        className={cn('menu-content', className)}
        {...props}
      />
    </ContextMenuPrimitive.Portal>
  );
}

function ContextMenuItem({
  className,
  ...props
}: ComponentProps<typeof ContextMenuPrimitive.Item>) {
  return <ContextMenuPrimitive.Item className={cn('menu-item', className)} {...props} />;
}

function ContextMenuSeparator({
  className,
  ...props
}: ComponentProps<typeof ContextMenuPrimitive.Separator>) {
  return <ContextMenuPrimitive.Separator className={cn('menu-separator', className)} {...props} />;
}

function DropdownMenuRoot(props: ComponentProps<typeof DropdownMenuPrimitive.Root>) {
  const state = useMenuState(props);
  return <DropdownMenuPrimitive.Root {...props} {...state} />;
}

function DropdownMenuContent({
  className,
  sideOffset = 4,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.Content>) {
  return (
    <DropdownMenuPrimitive.Portal>
      <DropdownMenuPrimitive.Content
        collisionPadding={8}
        sideOffset={sideOffset}
        className={cn('menu-content', className)}
        {...props}
      />
    </DropdownMenuPrimitive.Portal>
  );
}

function DropdownMenuItem({
  className,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.Item>) {
  return <DropdownMenuPrimitive.Item className={cn('menu-item', className)} {...props} />;
}

function DropdownMenuSeparator({
  className,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.Separator>) {
  return <DropdownMenuPrimitive.Separator className={cn('menu-separator', className)} {...props} />;
}

export const ContextMenu = {
  Root: ContextMenuRoot,
  Trigger: ContextMenuPrimitive.Trigger,
  Content: ContextMenuContent,
  Item: ContextMenuItem,
  Separator: ContextMenuSeparator,
};

export const DropdownMenu = {
  Root: DropdownMenuRoot,
  Trigger: DropdownMenuPrimitive.Trigger,
  Content: DropdownMenuContent,
  Item: DropdownMenuItem,
  Separator: DropdownMenuSeparator,
};
