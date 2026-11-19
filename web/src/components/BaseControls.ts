import { Button as Primitive } from "@ui-sdk/react/button";
import { type ComponentProps, createElement, type MouseEvent } from "react";

export { Input } from "@ui-sdk/react/input";
export { Textarea } from "@ui-sdk/react/textarea";

type ButtonProps = Omit<ComponentProps<typeof Primitive>, "onClick"> & {
  onClick?: (event: MouseEvent<HTMLButtonElement>) => void;
};

/**
 * Discards what `onClick` returns, so a returned Promise never turns the button pending,
 * and reports a thrown error instead of letting the primitive keep it in its state.
 */
export function Button({ onClick, ...props }: ButtonProps) {
  return createElement(Primitive, {
    ...props,
    onClick:
      onClick === undefined
        ? undefined
        : (event: MouseEvent<HTMLElement>) => {
            try {
              onClick(event as MouseEvent<HTMLButtonElement>);
            } catch (error) {
              reportError(error);
            }
          },
  });
}

/** A native form that leaves validation to its controls, as the forms here submit by hand. */
export function Form(props: ComponentProps<"form">) {
  return createElement("form", { noValidate: true, ...props });
}
