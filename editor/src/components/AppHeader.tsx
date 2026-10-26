import type { ReactNode } from 'react';
import pixauroMark from '../assets/pixauro-mark.svg';

export function AppHeader({ children, home = false }: { children?: ReactNode; home?: boolean }) {
  return (
    <header className={`app-titlebar${home ? ' home-titlebar' : ''}`}>
      <div className="brand">
        <img src={pixauroMark} width={18} height={18} alt="" />
        <strong>Pixauro</strong>
      </div>
      {children}
    </header>
  );
}
