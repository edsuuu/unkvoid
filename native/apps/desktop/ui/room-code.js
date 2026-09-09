/**
 * O código da sala. É a única chave que existe: quem tem, entra e vê a tela.
 *
 * Sorteado aqui mesmo, e não pedido ao servidor: a sala nasce quando alguém entra com um
 * código e acaba quando o último sai, então não há o que reservar. O que impede varrer
 * códigos é o teto de conexões por IP do SFU, não quem sorteou a string.
 */

/** 12 caracteres de a-z0-9 ≈ 4,7e18 combinações. Curto para colar no WhatsApp. */
export const CODE_LENGTH = 12;

const ALFABETO = 'abcdefghijklmnopqrstuvwxyz0123456789';

/** O maior múltiplo de 36 que cabe num byte. Acima disto, o byte é descartado. */
const LIMITE = ALFABETO.length * Math.floor(256 / ALFABETO.length);

export const isRoomCode = valor => new RegExp(`^[a-z0-9]{${CODE_LENGTH}}$`).test(valor);

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
