// Confirmaciones con el modal de la app (en vez del confirm() del navegador). Portado de Ethereon.
import { t } from "./i18n.svelte";

export interface Ask {
  title: string;
  body?: string;
  confirm?: string;
  danger?: boolean;
}

export interface Prompt extends Ask {
  placeholder?: string;
  secret?: boolean;
  /** Texto con el que arranca el campo (renombrar). */
  value?: string;
}

export const confirmState = $state({
  open: false,
  title: "",
  body: "",
  confirm: "OK",
  danger: false,
  /** Con input, el modal pide un texto (por ejemplo una contraseña). */
  input: null as null | { placeholder: string; secret: boolean },
  value: "",
  resolve: null as ((ok: boolean) => void) | null,
});

function open(o: Ask, input: Prompt | null, done: (ok: boolean) => void) {
  Object.assign(confirmState, {
    title: o.title,
    body: o.body ?? "",
    confirm: o.confirm ?? t("common.ok"),
    danger: o.danger ?? false,
    input: input ? { placeholder: input.placeholder ?? "", secret: input.secret ?? false } : null,
    value: input?.value ?? "",
    open: true,
    resolve: (ok: boolean) => {
      confirmState.open = false;
      done(ok);
    },
  });
}

export function ask(o: Ask): Promise<boolean> {
  return new Promise((resolve) => open(o, null, resolve));
}

/** Como ask, pero pide un texto. Devuelve null si se cancela. */
export function prompt(o: Prompt): Promise<string | null> {
  return new Promise((resolve) => open(o, o, (ok) => resolve(ok ? confirmState.value : null)));
}
