use std::sync::OnceLock;
use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
use windows::core::PCWSTR;

const RATE: u32 = 44100;

pub fn beep_wav(freq_hz: u32, ms: u32) -> Vec<u8> {
    let n = (RATE as u64 * ms as u64 / 1000) as usize;
    let data_len = (n * 2) as u32;
    let fade = (RATE as usize * 5 / 1000).max(1);
    let mut w = Vec::with_capacity(44 + n * 2);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data_len).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&1u16.to_le_bytes()); // mono
    w.extend_from_slice(&RATE.to_le_bytes());
    w.extend_from_slice(&(RATE * 2).to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..n {
        let t = i as f32 / RATE as f32;
        let env = (i as f32 / fade as f32)
            .min((n - 1 - i) as f32 / fade as f32)
            .min(1.0);
        let s =
            0.3 * i16::MAX as f32 * (2.0 * std::f32::consts::PI * freq_hz as f32 * t).sin() * env;
        w.extend_from_slice(&(s as i16).to_le_bytes());
    }
    w
}

pub fn play(muted: bool) {
    static MUTED: OnceLock<Vec<u8>> = OnceLock::new();
    static UNMUTED: OnceLock<Vec<u8>> = OnceLock::new();
    let wav = if muted {
        MUTED.get_or_init(|| beep_wav(440, 80))
    } else {
        UNMUTED.get_or_init(|| beep_wav(880, 80))
    };
    unsafe {
        let _ = PlaySoundW(
            PCWSTR(wav.as_ptr() as *const u16),
            None,
            SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_layout() {
        let w = beep_wav(440, 80);
        assert_eq!(w.len(), 44 + 3528 * 2);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(&w[8..12], b"WAVE");
        assert_eq!(u32::from_le_bytes(w[24..28].try_into().unwrap()), 44100);
        assert_eq!(i16::from_le_bytes([w[44], w[45]]), 0);
    }
}
