#[cfg(test)]
mod tests;

const DQLN: [u32; 16] = [
    2048, 4, 135, 213, 273, 323, 373, 425, 425, 373, 323, 273, 213, 135, 4, 2048,
];
const WI: [u32; 8] = [4084, 18, 41, 64, 112, 198, 355, 1122];
const FI: [u32; 8] = [0, 0, 0, 1, 1, 1, 3, 7];
const FLOAT_ZERO: u32 = 32;
const YU_RESET: u32 = 544;
const YL_RESET: u32 = 34_816;
pub(crate) const FULL_SCALE: f32 = 8_192.0;

#[derive(Clone, Copy, Debug)]
struct Prediction {
    se: u32,
    sez: u32,
    y: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct G726 {
    yu: u32,
    yl: u32,
    dms: u32,
    dml: u32,
    ap: u32,
    a: [u32; 2],
    b: [u32; 6],
    dq: [u32; 6],
    sr: [u32; 2],
    pk: [u32; 2],
    td: u32,
}

impl Default for G726 {
    fn default() -> Self {
        Self {
            yu: YU_RESET,
            yl: YL_RESET,
            dms: 0,
            dml: 0,
            ap: 0,
            a: [0; 2],
            b: [0; 6],
            dq: [FLOAT_ZERO; 6],
            sr: [FLOAT_ZERO; 2],
            pk: [0; 2],
            td: 0,
        }
    }
}

impl G726 {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn decode(&mut self, code: u8) -> i16 {
        let prediction = self.predict();
        self.update(u32::from(code & 0x0F), prediction) as u16 as i16
    }

    #[cfg(any(test, feature = "synth"))]
    pub fn encode(&mut self, sample: i16) -> u8 {
        self.encode_uniform((i32::from(sample) >> 2) as u32 & 16_383)
    }

    #[cfg(any(test, feature = "synth"))]
    fn encode_uniform(&mut self, sl: u32) -> u8 {
        let prediction = self.predict();
        let (dl, ds) = log(subta(sl, prediction.se));
        let code = quan(subtb(dl, prediction.y), ds);
        self.update(code, prediction);
        code as u8
    }

    fn predict(&self) -> Prediction {
        let wa1 = fmult(self.a[0], self.sr[0]);
        let wa2 = fmult(self.a[1], self.sr[1]);
        let sezi = self
            .b
            .iter()
            .zip(&self.dq)
            .fold(0, |sum, (&b, &dq)| (sum + fmult(b, dq)) & 65_535);
        let sei = (((sezi + wa2) & 65_535) + wa1) & 65_535;
        Prediction {
            se: sei >> 1,
            sez: sezi >> 1,
            y: mix(lima(self.ap), self.yu, self.yl),
        }
    }

    fn update(&mut self, code: u32, p: Prediction) -> u32 {
        let dqs = code >> 3;
        let dql = (DQLN[code as usize] + (p.y >> 2)) & 4_095;
        let dq = antilog(dql, dqs);
        let tr = trans(self.td, self.yl, dq);
        let fi = FI[magnitude(code)];
        let dmsp = filta(fi, self.dms);
        let dmlp = filtb(fi, self.dml);
        let yup = limb(filtd(WI[magnitude(code)], p.y));
        let ylp = filte(yup, self.yl);
        let (pk0, sigpk) = addc(dq, p.sez);
        let sr = addb(dq, p.se);
        let a2p = limc(upa2(
            pk0, self.pk[0], self.pk[1], self.a[1], self.a[0], sigpk,
        ));
        let a1p = limd(upa1(pk0, self.pk[0], self.a[0], sigpk), a2p);
        let tdp = tone(a2p);
        let ax = subtc(dmsp, dmlp, tdp, p.y);
        let app = filtc(ax, self.ap);
        for (b, &dqn) in self.b.iter_mut().zip(&self.dq) {
            *b = if tr { 0 } else { upb(xor(dqn, dq), *b, dq) };
        }
        self.a = if tr { [0, 0] } else { [a1p, a2p] };
        self.td = if tr { 0 } else { tdp };
        self.ap = if tr { 256 } else { app };
        self.yu = yup;
        self.yl = ylp;
        self.dms = dmsp;
        self.dml = dmlp;
        self.pk = [pk0, self.pk[0]];
        self.dq.rotate_right(1);
        self.dq[0] = float(dq & 32_767, (dq >> 15) & 1);
        self.sr = [float_signed(sr), self.sr[0]];
        sr
    }
}

fn magnitude(code: u32) -> usize {
    (if code >> 3 == 0 {
        code & 7
    } else {
        (15 - code) & 7
    }) as usize
}

fn exponent(magnitude: u32) -> u32 {
    u32::BITS - magnitude.leading_zeros()
}

fn float(magnitude: u32, sign: u32) -> u32 {
    let exp = exponent(magnitude);
    let mant = if magnitude == 0 {
        1 << 5
    } else {
        (magnitude << 6) >> exp
    };
    (sign << 10) + (exp << 6) + mant
}

fn float_signed(value: u32) -> u32 {
    let sign = value >> 15;
    let magnitude = if sign == 0 {
        value
    } else {
        (65_536 - value) & 32_767
    };
    float(magnitude, sign)
}

fn fmult(an: u32, srn: u32) -> u32 {
    let ans = an >> 15;
    let anmag = if ans == 0 {
        an >> 2
    } else {
        (16_384 - (an >> 2)) & 8_191
    };
    let anexp = exponent(anmag);
    let anmant = if anmag == 0 {
        1 << 5
    } else {
        (anmag << 6) >> anexp
    };
    let srns = srn >> 10;
    let srnexp = (srn >> 6) & 15;
    let srnmant = srn & 63;
    let wanexp = srnexp + anexp;
    let wanmant = (srnmant * anmant + 48) >> 4;
    let wanmag = if wanexp <= 26 {
        (wanmant << 7) >> (26 - wanexp)
    } else {
        ((wanmant << 7) << (wanexp - 26)) & 32_767
    };
    if srns ^ ans == 0 {
        wanmag
    } else {
        (65_536 - wanmag) & 65_535
    }
}

fn lima(ap: u32) -> u32 {
    if ap >= 256 { 64 } else { ap >> 2 }
}

fn mix(al: u32, yu: u32, yl: u32) -> u32 {
    let dif = (yu + 16_384 - (yl >> 6)) & 16_383;
    let negative = dif >> 13 != 0;
    let difm = if negative {
        (16_384 - dif) & 8_191
    } else {
        dif
    };
    let prodm = (difm * al) >> 6;
    let prod = if negative {
        (16_384 - prodm) & 16_383
    } else {
        prodm
    };
    ((yl >> 6) + prod) & 8_191
}

fn antilog(dql: u32, dqs: u32) -> u32 {
    let dex = (dql >> 7) & 15;
    let dqt = (dql & 127) + 128;
    let dqmag = if dql >> 11 != 0 {
        0
    } else {
        (dqt << 7) >> (14 - dex)
    };
    (dqs << 15) + dqmag
}

fn twos(dq: u32) -> u32 {
    if (dq >> 15) & 1 == 0 {
        dq & 65_535
    } else {
        (65_536 - (dq & 32_767)) & 65_535
    }
}

fn widen(value: u32) -> u32 {
    if value >> 14 == 0 {
        value
    } else {
        value + 32_768
    }
}

fn addb(dq: u32, se: u32) -> u32 {
    (twos(dq) + widen(se)) & 65_535
}

fn addc(dq: u32, sez: u32) -> (u32, bool) {
    let dqsez = (twos(dq) + widen(sez)) & 65_535;
    (dqsez >> 15, dqsez == 0)
}

fn trans(td: u32, yl: u32, dq: u32) -> bool {
    let ylint = yl >> 15;
    let ylfrac = (yl >> 10) & 31;
    let thr = if ylint > 9 {
        31_744
    } else {
        (ylfrac + 32) << ylint
    };
    let dqthr = (thr + (thr >> 1)) >> 1;
    (dq & 32_767) > dqthr && td == 1
}

fn filta(fi: u32, dms: u32) -> u32 {
    let dif = ((fi << 9) + 8_192 - dms) & 8_191;
    let difsx = if dif >> 12 == 0 {
        dif >> 5
    } else {
        (dif >> 5) + 3_840
    };
    (difsx + dms) & 4_095
}

fn filtb(fi: u32, dml: u32) -> u32 {
    let dif = ((fi << 11) + 32_768 - dml) & 32_767;
    let difsx = if dif >> 14 == 0 {
        dif >> 7
    } else {
        (dif >> 7) + 16_128
    };
    (difsx + dml) & 16_383
}

fn filtc(ax: u32, ap: u32) -> u32 {
    let dif = ((ax << 9) + 2_048 - ap) & 2_047;
    let difsx = if dif >> 10 == 0 {
        dif >> 4
    } else {
        (dif >> 4) + 896
    };
    (difsx + ap) & 1_023
}

fn filtd(wi: u32, y: u32) -> u32 {
    let dif = ((wi << 5) + 131_072 - y) & 131_071;
    let difsx = if dif >> 16 == 0 {
        dif >> 5
    } else {
        (dif >> 5) + 4_096
    };
    (y + difsx) & 8_191
}

fn limb(yut: u32) -> u32 {
    yut.clamp(544, 5_120)
}

fn filte(yup: u32, yl: u32) -> u32 {
    let dif = (yup + ((1_048_576 - yl) >> 6)) & 16_383;
    let difsx = if dif >> 13 == 0 { dif } else { dif + 507_904 };
    (yl + difsx) & 524_287
}

fn subtc(dmsp: u32, dmlp: u32, tdp: u32, y: u32) -> u32 {
    let dif = ((dmsp << 2) + 32_768 - dmlp) & 32_767;
    let difm = if dif >> 14 == 0 {
        dif
    } else {
        (32_768 - dif) & 16_383
    };
    u32::from(!(y >= 1_536 && difm < (dmlp >> 3) && tdp == 0))
}

fn tone(a2p: u32) -> u32 {
    u32::from((32_768..53_760).contains(&a2p))
}

fn limc(a2t: u32) -> u32 {
    const UPPER: u32 = 12_288;
    const LOWER: u32 = 53_248;
    if (32_768..=LOWER).contains(&a2t) {
        LOWER
    } else if (UPPER..=32_767).contains(&a2t) {
        UPPER
    } else {
        a2t
    }
}

fn limd(a1t: u32, a2p: u32) -> u32 {
    const OME: u32 = 15_360;
    let upper = (OME + 65_536 - a2p) & 65_535;
    let lower = (a2p + 65_536 - OME) & 65_535;
    if a1t >= 32_768 && a1t <= lower {
        lower
    } else if a1t >= upper && a1t <= 32_767 {
        upper
    } else {
        a1t
    }
}

fn upa1(pk0: u32, pk1: u32, a1: u32, sigpk: bool) -> u32 {
    let uga1 = match (sigpk, pk0 ^ pk1) {
        (true, _) => 0,
        (false, 0) => 192,
        (false, _) => 65_344,
    };
    let ash = a1 >> 8;
    let ula1 = if a1 >> 15 == 0 {
        65_536 - ash
    } else {
        65_536 - (ash + 65_280)
    } & 65_535;
    (a1 + ((uga1 + ula1) & 65_535)) & 65_535
}

fn upa2(pk0: u32, pk1: u32, pk2: u32, a2: u32, a1: u32, sigpk: bool) -> u32 {
    let uga2a = if pk0 ^ pk2 == 0 { 16_384 } else { 114_688 };
    let fa1 = if a1 >> 15 == 0 {
        a1.min(8_191) << 2
    } else if a1 >= 57_345 {
        (a1 << 2) & 131_071
    } else {
        24_577 << 2
    };
    let fa = if pk0 ^ pk1 != 0 {
        fa1
    } else {
        (131_072 - fa1) & 131_071
    };
    let uga2b = (uga2a + fa) & 131_071;
    let uga2 = match (sigpk, uga2b >> 16) {
        (true, _) => 0,
        (false, 0) => uga2b >> 7,
        (false, _) => (uga2b >> 7) + 64_512,
    };
    let ula2 = if a2 >> 15 == 0 {
        65_536 - (a2 >> 7)
    } else {
        65_536 - ((a2 >> 7) + 65_024)
    } & 65_535;
    (a2 + ((uga2 + ula2) & 65_535)) & 65_535
}

fn xor(dqn: u32, dq: u32) -> u32 {
    ((dq >> 15) & 1) ^ (dqn >> 10)
}

fn upb(u: u32, b: u32, dq: u32) -> u32 {
    let ugb = match (dq & 32_767 == 0, u) {
        (true, _) => 0,
        (false, 0) => 128,
        (false, _) => 65_408,
    };
    let ulb = if b >> 15 == 0 {
        65_536 - (b >> 8)
    } else {
        65_536 - ((b >> 8) + 65_280)
    } & 65_535;
    (b + ((ugb + ulb) & 65_535)) & 65_535
}

#[cfg(any(test, feature = "synth"))]
fn subta(sl: u32, se: u32) -> u32 {
    let sli = if sl >> 13 == 0 { sl } else { sl + 49_152 };
    (sli + 65_536 - widen(se)) & 65_535
}

#[cfg(any(test, feature = "synth"))]
fn log(d: u32) -> (u32, u32) {
    let ds = d >> 15;
    let dqm = if ds == 0 { d } else { (65_536 - d) & 32_767 };
    let exp = exponent(dqm).saturating_sub(1);
    let mant = ((dqm << 7) >> exp) & 127;
    ((exp << 7) + mant, ds)
}

#[cfg(any(test, feature = "synth"))]
fn subtb(dl: u32, y: u32) -> u32 {
    (dl + 4_096 - (y >> 2)) & 4_095
}

#[cfg(any(test, feature = "synth"))]
fn quan(dln: u32, ds: u32) -> u32 {
    let code = match dln {
        3_972.. => 1,
        2_048.. => 15,
        400.. => 7,
        349.. => 6,
        300.. => 5,
        246.. => 4,
        178.. => 3,
        80.. => 2,
        _ => 1,
    };
    let code = if ds != 0 { 15 - code } else { code };
    if code == 0 { 15 } else { code }
}
