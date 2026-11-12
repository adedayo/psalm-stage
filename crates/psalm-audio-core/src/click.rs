use std::f32::consts::TAU;

/// Synthesizes a crisp, zero-artifact metronome click directly at the active device sample rate.
/// Beat 1 receives a brighter pitch accent; Beats 2..N use a softer tone.
#[derive(Debug, Clone)]
pub struct ClickSynth {
    sample_rate: f32,
    phase: f32,
    env_samples_remaining: u32,
    env_total_samples: u32,
    freq_hz: f32,
    gain: f32,
}

impl ClickSynth {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate: sample_rate as f32,
            phase: 0.0,
            env_samples_remaining: 0,
            env_total_samples: 1,
            freq_hz: 1000.0,
            gain: 0.85,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.sample_rate = sample_rate as f32;
    }

    pub fn trigger(&mut self, is_downbeat: bool) {
        self.phase = 0.0;
        self.freq_hz = if is_downbeat { 1480.0 } else { 987.0 };
        self.gain = if is_downbeat { 0.95 } else { 0.65 };
        let dur_sec = if is_downbeat { 0.045 } else { 0.032 };
        let total = (self.sample_rate * dur_sec).max(1.0) as u32;
        self.env_total_samples = total;
        self.env_samples_remaining = total;
    }

    pub fn is_active(&self) -> bool {
        self.env_samples_remaining > 0
    }

    #[inline]
    pub fn next_sample(&mut self) -> f32 {
        if self.env_samples_remaining == 0 {
            return 0.0;
        }
        let progress = 1.0 - (self.env_samples_remaining as f32 / self.env_total_samples as f32);
        // Fast exponential decay for a percussive woodblock-like ping.
        let env = (-progress * 7.5).exp();
        let sample = (self.phase * TAU).sin() * env * self.gain;

        self.phase = (self.phase + self.freq_hz / self.sample_rate).fract();
        self.env_samples_remaining -= 1;
        sample
    }
}

/// Ringbuffer-based sample-accurate delay line to align metronome click with DSP stretch latency.
#[derive(Debug, Clone)]
pub struct ClickDelayBuffer {
    buffer: Vec<f32>,
    write_pos: usize,
}

impl ClickDelayBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: vec![0.0; capacity.max(64)],
            write_pos: 0,
        }
    }

    pub fn clear(&mut self) {
        self.buffer.fill(0.0);
        self.write_pos = 0;
    }

    #[inline]
    pub fn process_sample(&mut self, sample: f32, delay_samples: usize) -> f32 {
        if delay_samples == 0 {
            return sample;
        }
        let cap = self.buffer.len();
        let delay = delay_samples.min(cap - 1);
        let read_pos = (self.write_pos + cap - delay) % cap;
        let out = self.buffer[read_pos];
        self.buffer[self.write_pos] = sample;
        self.write_pos = (self.write_pos + 1) % cap;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_click_synth_trigger_and_decay() {
        let mut synth = ClickSynth::new(48000);
        assert!(!synth.is_active());
        assert_eq!(synth.next_sample(), 0.0);

        // Downbeat trigger
        synth.trigger(true);
        assert!(synth.is_active());
        assert_eq!(synth.freq_hz, 1480.0);

        let mut samples = Vec::new();
        while synth.is_active() {
            samples.push(synth.next_sample());
        }

        assert!(!samples.is_empty());
        assert!(samples.len() > 100);
        assert_eq!(synth.next_sample(), 0.0);

        // Offbeat trigger
        synth.trigger(false);
        assert!(synth.is_active());
        assert_eq!(synth.freq_hz, 987.0);
    }

    #[test]
    fn test_click_delay_buffer_exact_delay() {
        let mut delay_buf = ClickDelayBuffer::new(512);

        // Zero delay: pass-through immediately
        assert_eq!(delay_buf.process_sample(1.23, 0), 1.23);
        assert_eq!(delay_buf.process_sample(4.56, 0), 4.56);

        delay_buf.clear();

        // 10 samples delay
        let delay = 10;
        let mut out = Vec::new();
        for i in 0..30 {
            let sample = (i + 1) as f32;
            let res = delay_buf.process_sample(sample, delay);
            out.push(res);
        }

        // First 10 samples must be 0.0
        for i in 0..10 {
            assert_eq!(out[i], 0.0, "Sample at index {} should be 0.0", i);
        }

        // Sample at index 10 should be sample 1.0 (from i=0)
        assert_eq!(out[10], 1.0);
        assert_eq!(out[19], 10.0);

        // Test clear
        delay_buf.clear();
        assert_eq!(delay_buf.process_sample(9.0, 10), 0.0);
    }
}
