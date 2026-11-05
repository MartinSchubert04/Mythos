// Todos los textos en inglés (mismas claves que ../es).
import account from "./account";
import admin from "./admin";
import browse from "./browse";
import common from "./common";
import player from "./player";

export default { ...common, ...browse, ...player, ...account, ...admin };
