/**
 * O código da sala. É a única chave que existe: quem tem, entra e vê a tela.
 *
 * Sorteado aqui mesmo, e não pedido ao servidor: a sala nasce quando alguém entra com um
 * código e acaba quando o último sai, então não há o que reservar. O que impede varrer
 * códigos é o teto de conexões por IP do SFU, não quem sorteou a string.
 */

/** Códigos legíveis podem ser digitados nos dois ambientes; os sorteados continuam longos. */
export const CODE_LENGTH = 12;
export const ROOM_CODE_MIN = 3;
export const ROOM_CODE_MAX = 32;

const ALFABETO = 'abcdefghijklmnopqrstuvwxyz0123456789';

/** O maior múltiplo de 36 que cabe num byte. Acima disto, o byte é descartado. */
const LIMITE = ALFABETO.length * Math.floor(256 / ALFABETO.length);

export const isRoomCode = valor =>
    typeof valor === 'string'
    && valor.length >= ROOM_CODE_MIN
    && valor.length <= ROOM_CODE_MAX
    && /^[a-z0-9][a-z0-9-]*[a-z0-9]$/.test(valor);

export function newRoomCode() {
    const codigo = [];

    // Descarta o que sobra do último múltiplo de 36: sem isso as sete primeiras letras
    // sairiam mais vezes que as outras, e entropia é tudo o que protege a sala.
    while (codigo.length < CODE_LENGTH) {
        for (const byte of crypto.getRandomValues(new Uint8Array(CODE_LENGTH))) {
            if (byte < LIMITE && codigo.length < CODE_LENGTH) {
                codigo.push(ALFABETO[byte % ALFABETO.length]);
            }
        }
    }

    return codigo.join('');
}
