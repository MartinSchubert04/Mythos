// Avisos que entran desde abajo.
import { errText } from "./api";

export const toasts = $state<{ id: number; text: string; bad: boolean }[]>([]);
let next = 1;

export function toast(text: string, bad = false) {
  const id = next++;
  toasts.push({ id, text, bad });
  setTimeout(
    () => {
      const i = toasts.findIndex((t) => t.id === id);
      if (i >= 0) toasts.splice(i, 1);
    },
    bad ? 5000 : 3000,
  );
}

/** Corre algo y avisa si falla (y con `ok`, si sale bien). Devuelve si salio bien. */
export async function attempt(f: () => Promise<unknown>, ok?: string): Promise<boolean> {
  try {
    await f();
    if (ok) toast(ok);
    return true;
  } catch (e) {
    toast(errText(e), true);
    return false;
  }
}
