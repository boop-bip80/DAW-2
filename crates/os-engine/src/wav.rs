// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Minimal WAV writer (32-bit float, stereo).

use std::io::{self, Write};

/// Writes interleaved stereo `f32` samples as a 32-bit IEEE float WAV file.
pub fn write_f32_stereo(w: &mut impl Write, rate: u32, interleaved: &[f32]) -> io::Result<()> {
    if interleaved.len() % 2 != 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "incomplete stereo frame"));
    }
    write_f32_stereo_header(w, rate, (interleaved.len() / 2) as u64)?;
    for s in interleaved {
        w.write_all(&s.to_le_bytes())?;
    }
    Ok(())
}

/// The header of a 32-bit IEEE float stereo WAV file of `frames` frames at `rate`; the samples follow it.
pub fn write_f32_stereo_header(w: &mut impl Write, rate: u32, frames: u64) -> io::Result<()> {
    const CHANNELS: u16 = 2;
    const BITS: u16 = 32;
    let data_len = frames.checked_mul(8)
        .and_then(|n| u32::try_from(n).ok())
        .filter(|&n| n <= u32::MAX - 36)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "take exceeds the 4 GiB RIFF WAV limit; raw recording must be retained for recovery"))?;
    let block_align = CHANNELS * BITS / 8;
    let byte_rate = rate.checked_mul(u32::from(block_align)).filter(|_| rate > 0)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid WAV sample rate"))?;
    w.write_all(b"RIFF")?;
    w.write_all(&(36 + data_len).to_le_bytes())?;
    w.write_all(b"WAVEfmt ")?;
    w.write_all(&16u32.to_le_bytes())?;
    w.write_all(&3u16.to_le_bytes())?; // WAVE_FORMAT_IEEE_FLOAT
    w.write_all(&CHANNELS.to_le_bytes())?;
    w.write_all(&rate.to_le_bytes())?;
    w.write_all(&byte_rate.to_le_bytes())?;
    w.write_all(&block_align.to_le_bytes())?;
    w.write_all(&BITS.to_le_bytes())?;
    w.write_all(b"data")?;
    w.write_all(&data_len.to_le_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn riff_limits_are_checked_before_writing() {
        let limit = u64::from(u32::MAX - 36) / 8;
        let mut b = Vec::new();
        write_f32_stereo_header(&mut b, 48000, limit).unwrap();
        assert_eq!(u32::from_le_bytes(b[40..44].try_into().unwrap()), (limit * 8) as u32);
        for frames in [limit + 1, u64::MAX / 8 + 1, u64::MAX] {
            b.clear();
            assert!(write_f32_stereo_header(&mut b, 48000, frames).is_err());
            assert!(b.is_empty());
        }
        for rate in [0, u32::MAX] {
            b.clear();
            assert!(write_f32_stereo_header(&mut b, rate, 1).is_err());
            assert!(b.is_empty());
        }
        b.clear();
        assert!(write_f32_stereo(&mut b, 48000, &[1.0]).is_err());
        assert!(b.is_empty());
    }
}
