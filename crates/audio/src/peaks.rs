//! Waveform summaries: the lowest and highest sample of each block of frames, so a zoomed-out
//! timeline can draw a long sound without touching every sample. A short span is read from the
//! samples, so a close view shows the shape inside a block (ADR 0122).

use crate::Pcm;

#[derive(Clone, Debug, PartialEq)]
pub struct Peaks {
    min: Vec<f32>,
    max: Vec<f32>,
    frames: usize,
}

impl Peaks {
    /// Frames per block. Short enough that a zoomed-out wave still shows the shape.
    pub const BLOCK: usize = 32;
    /// A column this many frames or shorter is read from the samples, not the blocks.
    const DETAIL: usize = 4096;

    /// The peaks of `pcm` (both channels together).
    pub fn of(pcm: &Pcm) -> Peaks {
        let ch = usize::from(pcm.channels.max(1));
        let (mut min, mut max) = (Vec::new(), Vec::new());
        for block in pcm.samples.chunks(Self::BLOCK * ch) {
            let (lo, hi) = block.iter().fold((0.0f32, 0.0f32), |(lo, hi), &s| (lo.min(s), hi.max(s)));
            min.push(lo);
            max.push(hi);
        }
        Peaks { min, max, frames: pcm.frames() }
    }

    /// The peaks of one channel of `pcm` (0 left, 1 right; a mono sound's one channel for
    /// either), for drawing the channels apart (the envelope editor).
    pub fn of_channel(pcm: &Pcm, channel: usize) -> Peaks {
        let ch = usize::from(pcm.channels.max(1));
        let c = channel.min(ch - 1);
        let (mut min, mut max) = (Vec::new(), Vec::new());
        for block in pcm.samples.chunks(Self::BLOCK * ch) {
            let (lo, hi) = block.iter().skip(c).step_by(ch).fold((0.0f32, 0.0f32), |(lo, hi), &s| (lo.min(s), hi.max(s)));
            min.push(lo);
            max.push(hi);
        }
        Peaks { min, max, frames: pcm.frames() }
    }

    /// Frames of the sound summarised.
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// Lowest and highest sample among frames `a..b` (to block precision); `None` outside the
    /// sound.
    pub fn range(&self, a: usize, b: usize) -> Option<(f32, f32)> {
        let b = b.min(self.frames);
        if a >= b {
            return None;
        }
        let (i, j) = (a / Self::BLOCK, (b - 1) / Self::BLOCK + 1);
        let lo = self.min.get(i..j)?.iter().fold(0.0f32, |m, &v| m.min(v));
        let hi = self.max.get(i..j)?.iter().fold(0.0f32, |m, &v| m.max(v));
        Some((lo, hi))
    }

    /// Lowest and highest sample in frames `a..b`, both channels together.
    /// A short window is the samples themselves. A long one is the block peaks.
    pub fn extent(&self, pcm: &Pcm, a: usize, b: usize) -> Option<(f32, f32)> {
        self.extent_of(pcm, None, a, b)
    }

    /// Like [`Self::extent`] for one channel (0 left, 1 right; mono answers either).
    pub fn extent_channel(&self, pcm: &Pcm, channel: usize, a: usize, b: usize) -> Option<(f32, f32)> {
        self.extent_of(pcm, Some(channel), a, b)
    }

    fn extent_of(&self, pcm: &Pcm, channel: Option<usize>, a: usize, b: usize) -> Option<(f32, f32)> {
        let b = b.min(self.frames).min(pcm.frames());
        if a >= b {
            return None;
        }
        if b - a <= Self::DETAIL {
            Some(sample_extent(pcm, channel, a, b))
        } else {
            self.range(a, b)
        }
    }
}

/// Min and max of frames `a..b`. `channel` is one channel, or every channel together.
fn sample_extent(pcm: &Pcm, channel: Option<usize>, a: usize, b: usize) -> (f32, f32) {
    let ch = usize::from(pcm.channels.max(1));
    let (mut lo, mut hi) = (0.0f32, 0.0f32);
    match channel {
        Some(channel) => {
            let c = channel.min(ch - 1);
            for frame in a..b {
                let i = frame.saturating_mul(ch).saturating_add(c);
                if let Some(&s) = pcm.samples.get(i) {
                    lo = lo.min(s);
                    hi = hi.max(s);
                }
            }
        }
        None => {
            let start = a.saturating_mul(ch);
            let end = b.saturating_mul(ch).min(pcm.samples.len());
            if let Some(slice) = pcm.samples.get(start..end) {
                for &s in slice {
                    lo = lo.min(s);
                    hi = hi.max(s);
                }
            }
        }
    }
    (lo, hi)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn peaks_cover_both_channels_by_block() {
        let mut s = vec![0.0f32; 2 * 600];
        s[2 * 10] = 0.75; // left, early
        s[2 * 300 + 1] = -0.5; // right, later
        let p = Peaks::of(&Pcm::new(8000, 2, s).unwrap());
        assert_eq!(p.frames(), 600);
        assert_eq!(p.range(0, 256), Some((0.0, 0.75)));
        assert_eq!(p.range(256, 512), Some((-0.5, 0.0)));
        assert_eq!(p.range(0, 600), Some((-0.5, 0.75)));
        assert_eq!(p.range(512, 600), Some((0.0, 0.0)));
        assert_eq!(p.range(600, 900), None);
        assert_eq!(p.range(5, 5), None);
    }

    #[test]
    fn a_close_view_reads_the_samples_and_a_long_one_uses_the_blocks() {
        let mut s = vec![0.0f32; 5000];
        s[5] = 0.8;
        s[4500] = -0.4;
        let pcm = Pcm::new(8000, 1, s).unwrap();
        let p = Peaks::of(&pcm);
        // The spike shares a block with the later frames, so the block summary smears it.
        assert_eq!(p.range(20, 30), Some((0.0, 0.8)));
        // The same window, read from the samples, is silence.
        assert_eq!(p.extent(&pcm, 20, 30), Some((0.0, 0.0)));
        assert_eq!(p.extent(&pcm, 0, 10), Some((0.0, 0.8)));
        // One channel of a mono sound is that sound.
        assert_eq!(p.extent_channel(&pcm, 1, 0, 10), Some((0.0, 0.8)));
        // Longer than the detail window: the block peaks, which still see both spikes.
        assert_eq!(p.extent(&pcm, 0, 5000), p.range(0, 5000));
        assert_eq!(p.extent(&pcm, 0, 5000), Some((-0.4, 0.8)));
        assert_eq!(p.extent(&pcm, 5000, 6000), None);
    }
}
