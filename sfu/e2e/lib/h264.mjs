import { BitReader } from './BitReader.mjs';
import { BitWriter } from './BitWriter.mjs';

/**
 * O H.264 sintético do harness. O quadro-chave vem pronto do ffmpeg (um IDR por
 * resolução); os quadros P são fatias `P_Skip` montadas aqui, com o `frame_num` certo, que
 * qualquer decodificador aceita e que repetem a imagem anterior. Cada quadro leva um SEI
 * `user_data_unregistered` com o contador e a hora de envio — é por ele que quem assiste
 * prova ordem, buraco e atraso —, e enchimento (NAL 12) até o tamanho da taxa pedida, para
 * o número de pacotes ser o de uma transmissão de verdade.
 */

export const NAL = { SLICE: 1, IDR: 5, SEI: 6, SPS: 7, PPS: 8, FILLER: 12, STAP_A: 24, FU_A: 28 };

const HIGH_PROFILES = new Set([100, 110, 122, 244, 44, 83, 86, 118, 128, 138, 139, 134, 135]);

const SEI_UUID = Buffer.from('756e6b766f69642d6532652d636f756e', 'hex');

const SEI_BODY = 16;

export const nalType = nal => nal[0] & 0x1f;

export const splitAnnexB = stream => {
    const nals = [];
    let start = -1;
    let index = 0;

    while (index + 3 <= stream.length) {
        const three = stream[index] === 0 && stream[index + 1] === 0 && stream[index + 2] === 1;

        if (!three) {
            index += 1;
            continue;
        }

        if (start >= 0) {
            let end = index;

            while (end > start && stream[end - 1] === 0) {
                end -= 1;
            }

            nals.push(stream.subarray(start, end));
        }

        index += 3;
        start = index;
    }

    if (start >= 0 && start < stream.length) {
        nals.push(stream.subarray(start));
    }

    return nals;
};

export const toAnnexB = nals => Buffer.concat(nals.flatMap(nal => [Buffer.from([0, 0, 0, 1]), nal]));

/** Tira os bytes de prevenção (`00 00 03`) para ler o RBSP. */
export const unescape = nal => {
    const output = [];
    let zeros = 0;

    for (let index = 1; index < nal.length; index += 1) {
        const byte = nal[index];

        if (zeros >= 2 && byte === 3) {
            zeros = 0;
            continue;
        }

        output.push(byte);
        zeros = byte === 0 ? zeros + 1 : 0;
    }

    return Buffer.from(output);
};

/** O inverso: um NAL com cabeçalho e o RBSP protegido contra o falso início de código. */
export const escape = (header, rbsp) => {
    const output = [header];
    let zeros = 0;

    for (const byte of rbsp) {
        if (zeros >= 2 && byte <= 3) {
            output.push(3);
            zeros = 0;
        }

        output.push(byte);
        zeros = byte === 0 ? zeros + 1 : 0;
    }

    return Buffer.from(output);
};

export const parseSps = nal => {
    const reader = new BitReader(unescape(nal));
    const profile = reader.bits(8);

    reader.skip(8);

    const level = reader.bits(8);
    const id = reader.unsigned();
    let chromaFormat = 1;
    let separateColourPlane = 0;

    if (HIGH_PROFILES.has(profile)) {
        chromaFormat = reader.unsigned();

        if (chromaFormat === 3) {
            separateColourPlane = reader.bit();
        }

        reader.unsigned();
        reader.unsigned();
        reader.skip(1);

        if (reader.bit()) {
            for (let list = 0; list < (chromaFormat === 3 ? 12 : 8); list += 1) {
                if (reader.bit()) {
                    skipScalingList(reader, list < 6 ? 16 : 64);
                }
            }
        }
    }

    const log2MaxFrameNum = reader.unsigned() + 4;
    const pocType = reader.unsigned();
    let log2MaxPocLsb = 0;

    if (pocType === 0) {
        log2MaxPocLsb = reader.unsigned() + 4;
    } else if (pocType === 1) {
        reader.skip(1);
        reader.signed();
        reader.signed();

        const cycle = reader.unsigned();

        for (let index = 0; index < cycle; index += 1) {
            reader.signed();
        }
    }

    reader.unsigned();
    reader.skip(1);

    const widthInMbs = reader.unsigned() + 1;
    const heightInMapUnits = reader.unsigned() + 1;
    const frameMbsOnly = reader.bit();

    if (!frameMbsOnly) {
        reader.skip(1);
    }

    reader.skip(1);

    let crop = { left: 0, right: 0, top: 0, bottom: 0 };

    if (reader.bit()) {
        crop = { left: reader.unsigned(), right: reader.unsigned(), top: reader.unsigned(), bottom: reader.unsigned() };
    }

    const heightInMbs = heightInMapUnits * (2 - frameMbsOnly);
    const chromaArray = separateColourPlane ? 0 : chromaFormat;
    const cropX = chromaArray === 0 ? 1 : chromaFormat === 3 ? 1 : 2;
    const cropY = (chromaArray === 0 ? 1 : chromaFormat === 1 ? 2 : 1) * (2 - frameMbsOnly);

    return {
        id,
        profile,
        level,
        log2MaxFrameNum,
        pocType,
        log2MaxPocLsb,
        frameMbsOnly,
        macroblocks: widthInMbs * heightInMbs,
        width: widthInMbs * 16 - cropX * (crop.left + crop.right),
        height: heightInMbs * 16 - cropY * (crop.top + crop.bottom),
    };
};

export const parsePps = nal => {
    const reader = new BitReader(unescape(nal));
    const id = reader.unsigned();
    const spsId = reader.unsigned();
    const cabac = reader.bit();
    const bottomFieldPocPresent = reader.bit();
    const sliceGroups = reader.unsigned() + 1;

    if (sliceGroups > 1) {
        throw new Error('slice groups are not supported by the synthetic encoder');
    }

    reader.unsigned();
    reader.unsigned();

    const weightedPrediction = reader.bit();

    reader.skip(2);
    reader.signed();
    reader.signed();
    reader.signed();

    const deblockingControl = reader.bit();

    reader.skip(1);

    const redundantPictureCount = reader.bit();

    return { id, spsId, cabac, bottomFieldPocPresent, weightedPrediction, deblockingControl, redundantPictureCount };
};

/**
 * Um quadro P inteiro de macroblocos pulados: repete a referência, custa meia dúzia de
 * bytes e é H.264 válido para qualquer decodificador. Só CAVLC (perfil baseline, o
 * `42e01f` que o app declara).
 */
export const buildSkipSlice = (sps, pps, frameNumber) => {
    if (pps.cabac || pps.weightedPrediction) {
        throw new Error('the synthetic P frame needs CAVLC without weighted prediction');
    }

    const writer = new BitWriter();

    writer.unsigned(0);
    writer.unsigned(5);
    writer.unsigned(pps.id);
    writer.bits(frameNumber % 2 ** sps.log2MaxFrameNum, sps.log2MaxFrameNum);

    if (!sps.frameMbsOnly) {
        writer.bit(0);
    }

    if (sps.pocType === 0) {
        writer.bits((frameNumber * 2) % 2 ** sps.log2MaxPocLsb, sps.log2MaxPocLsb);

        if (pps.bottomFieldPocPresent) {
            writer.signed(0);
        }
    }

    if (sps.pocType === 1) {
        throw new Error('pic_order_cnt_type 1 is not supported by the synthetic encoder');
    }

    if (pps.redundantPictureCount) {
        writer.unsigned(0);
    }

    writer.bit(0);
    writer.bit(0);
    writer.bit(0);
    writer.signed(0);

    if (pps.deblockingControl) {
        writer.unsigned(1);
    }

    writer.unsigned(sps.macroblocks);

    return escape(0x41, writer.finish());
};

/** O contador do quadro, a hora de envio e a resolução, num SEI que o decodificador ignora. */
export const buildCounterSei = ({ index, sentAt, width, height }) => {
    const body = Buffer.alloc(SEI_BODY);

    body.writeUInt32BE(index >>> 0, 0);
    body.writeDoubleBE(sentAt, 4);
    body.writeUInt16BE(width, 12);
    body.writeUInt16BE(height, 14);

    return escape(0x06, Buffer.concat([Buffer.from([5, SEI_UUID.length + SEI_BODY]), SEI_UUID, body, Buffer.from([0x80])]));
};

export const readCounterSei = nal => {
    const rbsp = unescape(nal);

    if (rbsp[0] !== 5 || rbsp[1] !== SEI_UUID.length + SEI_BODY || !rbsp.subarray(2, 18).equals(SEI_UUID)) {
        return null;
    }

    const body = rbsp.subarray(18, 18 + SEI_BODY);

    return {
        index: body.readUInt32BE(0),
        sentAt: body.readDoubleBE(4),
        width: body.readUInt16BE(12),
        height: body.readUInt16BE(14),
    };
};

export const buildFiller = size => {
    const filler = Buffer.alloc(Math.max(2, size), 0xff);

    filler[0] = 0x0c;
    filler[filler.length - 1] = 0x80;

    return filler;
};

const skipScalingList = (reader, size) => {
    let last = 8;
    let next = 8;

    for (let index = 0; index < size; index += 1) {
        if (next !== 0) {
            next = (last + reader.signed() + 256) % 256;
        }

        last = next === 0 ? last : next;
    }
};
