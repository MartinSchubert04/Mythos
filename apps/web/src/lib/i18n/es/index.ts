// Todos los textos en español. El español es la fuente de las claves (lib/i18n.svelte.ts).
import account from "./account";
import admin from "./admin";
import browse from "./browse";
import common from "./common";
import player from "./player";

export default { ...common, ...browse, ...player, ...account, ...admin };
