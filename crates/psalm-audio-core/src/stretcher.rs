use signalsmith_stretch::Stretch;

/// Stereo real-time pitch & time stretcher wrapping `signalsmith_stretch::Stretch`.
pub struct StemStretcher {
    stretch: Stretch,
    sample_rate: u32,
    semitones: f32,
}

impl StemStretcher {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            stretch: Stretch::preset_default(2, sample_rate),
            sample_rate,
            semitones: 0.0,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        if self.sample_rate != sample_rate {
            self.sample_rate = sample_rate;
            self.stretch = Stretch::preset_default(2, sample_rate);
            self.stretch
                .set_transpose_factor_semitones(self.semitones, None);
        }
    }

    pub fn set_semitones(&mut self, semitones: f32) {
        if (self.semitones - semitones).abs() > f32::EPSILON {
            self.semitones = semitones;
            self.stretch.set_transpose_factor_semitones(semitones, None);
        }
    }

    pub fn reset_with_preroll(&mut self, preroll_interleaved: &[f32], playback_rate: f64) {
        self.stretch.reset();
        if !preroll_interleaved.is_empty() {
            self.stretch.seek(preroll_interleaved, playback_rate);
        }
    }

    pub fn input_latency_frames(&self) -> usize {
        self.stretch.input_latency()
    }

    pub fn output_latency_frames(&self) -> usize {
        self.stretch.output_latency()
    }

    pub fn total_latency_frames(&self) -> usize {
        self.input_latency_frames() + self.output_latency_frames()
    }

    /// Processes `input_interleaved` (stereo `f32`) into `output_interleaved` (stereo `f32`).
    /// Time stretching happens automatically when `input.len() != output.len()`.
    pub fn process(&mut self, input_interleaved: &[f32], output_interleaved: &mut [f32]) {
        self.stretch.process(input_interleaved, output_interleaved);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stretcher_latency() {
        let sample_rate = 48000;
        let st = StemStretcher::new(sample_rate);
        assert!(st.input_latency_frames() > 0);
        assert!(st.output_latency_frames() > 0);
        assert_eq!(
            st.total_latency_frames(),
            st.input_latency_frames() + st.output_latency_frames()
        );
    }
}
