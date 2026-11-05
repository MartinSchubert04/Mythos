// Tipos de los diccionarios (ver lib/i18n.svelte.ts).

/** Un texto que cambia con la cantidad: `n === 1` usa `one`, si no `other`. */
export type Plural = { one: string; other: string };
export type Msg = string | Plural;

/** Lo que tiene que tener la traduccion de un archivo en español: las mismas claves (ni una mas
 *  ni una menos), con texto donde el español tiene texto y plural donde tiene plural. Se usa con
 *  `satisfies Shape<typeof es>`. */
export type Shape<T> = { [K in keyof T]: T[K] extends string ? string : Plural };
