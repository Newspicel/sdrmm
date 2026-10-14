import { type ReactElement, type ReactNode, useState } from "react";
import { advance, gone, heldFirst, shown } from "./presence";

export function RevealRows({
  children,
  className,
}: {
  children: readonly ReactElement[];
  className?: string;
}) {
  const [held, setHeld] = useState(() => heldFirst(children));
  if (held.last !== children) {
    setHeld(advance(held, children));
  }
  return shown(children, held).map((row) => (
    <Reveal
      key={row.key}
      enter={held.entered.has(row.key)}
      leaving={row.leaving}
      className={className}
      onGone={() => setHeld((current) => gone(current, row.key))}
    >
      {row.element}
    </Reveal>
  ));
}

function Reveal({
  enter,
  leaving,
  className,
  onGone,
  children,
}: {
  enter: boolean;
  leaving: boolean;
  className?: string;
  onGone: () => void;
  children: ReactNode;
}) {
  return (
    <div
      className="reveal"
      data-enter={enter || undefined}
      data-leaving={leaving || undefined}
      onAnimationEnd={(event) => {
        if (leaving && event.target === event.currentTarget) {
          onGone();
        }
      }}
    >
      <div className={className}>{children}</div>
    </div>
  );
}
