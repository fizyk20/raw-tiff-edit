use std::cmp;

use byteorder::{ByteOrder, LittleEndian};

#[derive(Debug, Clone)]
pub struct LookupTable {
    table: Vec<(u16, u16, u16)>,
}

impl LookupTable {
    pub fn new(table: &[u16]) -> LookupTable {
        let mut tbl = vec![(0, 0, 0); table.len()];
        for i in 0..table.len() {
            let center = table[i];
            let lower = if i > 0 { table[i - 1] } else { center };
            let upper = if i < (table.len() - 1) {
                table[i + 1]
            } else {
                center
            };
            let base = if center == 0 {
                0
            } else {
                center - ((upper - lower + 2) / 4)
            };
            let delta = upper - lower;
            tbl[i] = (center, base, delta);
        }
        LookupTable { table: tbl }
    }

    #[inline(always)]
    pub fn dither(&self, value: u16, rand: &mut u32) -> u16 {
        let (_, sbase, sdelta) = self.table[value as usize];
        let base = sbase as u32;
        let delta = sdelta as u32;
        let pixel = base + ((delta * (*rand & 2047) + 1024) >> 12);
        *rand = 15700 * (*rand & 65535) + (*rand >> 16);
        pixel as u16
    }

    #[inline(always)]
    pub fn reverse_lookup(&self, value: u16) -> u16 {
        let start_index = match self.table.binary_search_by_key(&value, |entry| entry.1) {
            Ok(i) => i,
            Err(i) => i,
        }
        .saturating_sub(RANGE);
        const RANGE: usize = 2;
        let end_index = cmp::min(start_index + 2 * RANGE, self.table.len() - 1);
        let entry = self.table[start_index..end_index]
            .iter()
            .enumerate()
            .min_by_key(|(_, (center, _, _))| {
                let center = *center as i32;
                (center - value as i32).abs()
            })
            .map(|(i, _)| i + start_index)
            .unwrap();
        entry as u16
    }
}

pub fn calculate_curve() -> LookupTable {
    let centry = [8000, 10400, 12900, 14100];
    let mut curve: [usize; 6] = [0, 0, 0, 0, 0, 4095];

    for i in 0..4 {
        curve[i + 1] = ((centry[i] >> 2) & 0xfff) as usize;
    }

    let mut out = vec![0 as u16; curve[5] + 1];
    for i in 0..5 {
        for j in (curve[i] + 1)..(curve[i + 1] + 1) {
            out[j] = out[j - 1] + (1 << i);
        }
    }

    LookupTable::new(&out)
}

#[derive(Debug, Copy, Clone)]
pub struct BitPumpLSB<'a> {
    buffer: &'a [u8],
    pos: usize,
    bits: u64,
    nbits: u32,
}

impl<'a> BitPumpLSB<'a> {
    pub fn new(src: &'a [u8]) -> BitPumpLSB {
        BitPumpLSB {
            buffer: src,
            pos: 0,
            bits: 0,
            nbits: 0,
        }
    }

    #[inline(always)]
    pub fn peek_bits(&mut self, num: u32) -> u32 {
        if num > self.nbits {
            let inbits: u64 = LEu32(self.buffer, self.pos) as u64;
            self.bits = ((inbits << 32) | (self.bits << (32 - self.nbits))) >> (32 - self.nbits);
            self.pos += 4;
            self.nbits += 32;
        }
        (self.bits & (0x0ffffffffu64 >> (32 - num))) as u32
    }

    #[inline(always)]
    pub fn consume_bits(&mut self, num: u32) {
        self.nbits -= num;
        self.bits >>= num;
    }

    #[inline(always)]
    fn get_bits(&mut self, num: u32) -> u32 {
        if num == 0 {
            return 0;
        }

        let val = self.peek_bits(num);
        self.consume_bits(num);

        val
    }
}

#[allow(non_snake_case)]
#[inline]
pub fn LEu32(buf: &[u8], pos: usize) -> u32 {
    LittleEndian::read_u32(&buf[pos..pos + 4])
}

struct ReverseBitPump {
    data: Vec<u8>,
    bits: u64,
    n_bits: u8,
}

impl ReverseBitPump {
    fn new() -> Self {
        Self {
            data: vec![],
            bits: 0,
            n_bits: 0,
        }
    }

    fn push_bits(&mut self, val: u32, n_bits: u8) {
        self.bits |= (val as u64 & (0xFFFFFFFF >> (32 - n_bits))) << self.n_bits;
        self.n_bits += n_bits;
        while self.n_bits >= 8 {
            let byte = self.bits & 0xFF;
            self.n_bits -= 8;
            self.bits >>= 8;
            self.data.push(byte as u8);
        }
    }

    fn into_data(self) -> Vec<u8> {
        assert_eq!(self.n_bits, 0);
        self.data
    }
}

pub fn decode_arw2(buf: &[u8], width: usize, height: usize) -> Vec<u16> {
    let curve = calculate_curve();
    let mut result: Vec<u16> = vec![0; width * height];

    for (row, out) in result.chunks_mut(width).enumerate() {
        let mut pump = BitPumpLSB::new(&buf[(row * width)..]);

        let mut random = pump.peek_bits(16);
        for out in out.chunks_mut(32) {
            // Process 32 pixels at a time in interleaved fashion
            for j in 0..2 {
                let max = pump.get_bits(11);
                let min = pump.get_bits(11);
                let delta = max - min;
                // Calculate the size of the data shift needed by how large the delta is
                // A delta with 11 bits requires a shift of 4, 10 bits of 3, etc
                let delta_shift: u32 =
                    cmp::max(0, (32 - (delta.leading_zeros() as i32)) - 7) as u32;
                let imax = pump.get_bits(4) as usize;
                let imin = pump.get_bits(4) as usize;

                for i in 0..16 {
                    let val = if i == imax {
                        max
                    } else if i == imin {
                        min
                    } else {
                        cmp::min(0x7ff, (pump.get_bits(7) << delta_shift) + min)
                    };
                    out[j + (i * 2)] = curve.dither((val << 1) as u16, &mut random);
                }
            }
        }
    }

    result
}

pub fn encode_arw2(img: &[u16], width: usize) -> Vec<u8> {
    let curve = calculate_curve();
    let mut result: Vec<u8> = vec![];

    for input in img.chunks(width) {
        for input in input.chunks(32) {
            let mut pump = ReverseBitPump::new();
            let vals: Vec<_> = input
                .iter()
                .map(|value| curve.reverse_lookup(*value) >> 1)
                .collect();
            for j in 0..2 {
                let (mut imax, max) = vals
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i % 2 == j)
                    .max_by_key(|v| v.1)
                    .unwrap();
                let (mut imin, min) = vals
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i % 2 == j)
                    .min_by_key(|v| v.1)
                    .unwrap();
                imax /= 2;
                imin /= 2;
                if imax == imin && imin > 0 {
                    imin -= 1;
                } else if imax == imin {
                    imin += 1;
                }
                pump.push_bits((max & 0x7ff) as u32, 11);
                pump.push_bits((min & 0x7ff) as u32, 11);
                let delta = max - min;
                let delta_shift: u32 =
                    cmp::max(0, (16 - (delta.leading_zeros() as i32)) - 7) as u32;
                pump.push_bits((imax & 0xf) as u32, 4);
                pump.push_bits((imin & 0xf) as u32, 4);

                for i in 0..16 {
                    if i != imax && i != imin {
                        let val = vals[2 * i + j];
                        let val = (val - min) >> delta_shift;
                        pump.push_bits((val & 0x7f) as u32, 7);
                    }
                }
            }
            let result_row = pump.into_data();
            result.extend(result_row);
        }
    }

    result
}
