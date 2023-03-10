use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use bitflags::bitflags;
use log::{info, warn};

pub struct APU {
    output_buffer: Option<SampleBuffer>,

    square_wave1: SquareWave,
    square_wave2: SquareWave,
    triangle_wave: TriangleWave,

    /// Which channels the game wants enabled currently.
    guest_enabled_channels: AudioChannels,
    /// The user can override to mute a channel that the game has enabled.
    host_enabled_channels: AudioChannels,

    sq1_samples: Vec<f32>,
    sq2_samples: Vec<f32>,
    tri_samples: Vec<f32>,
    mixed_samples: Vec<f32>,

    last_cpu_cycles: u64,
}

bitflags! {
    pub struct AudioChannels : u8 {
        const SQUARE1 = 0x01;
        const SQUARE2 = 0x02;
        const TRIANGLE = 0x04;
        const NOISE = 0x08;
        const DMC = 0x10;
    }
}

pub struct SampleBuffer {
    buffer: Arc<Mutex<VecDeque<f32>>>,
    samples_per_second: u32,
}

impl SampleBuffer {
    pub fn new(freq: u32) -> SampleBuffer {
        SampleBuffer {
            buffer: Arc::new(Mutex::new(VecDeque::new())),
            samples_per_second: freq,
        }
    }

    pub fn clone_ref(&self) -> SampleBuffer {
        SampleBuffer {
            buffer: self.buffer.clone(),
            samples_per_second: self.samples_per_second,
        }
    }

    pub fn output_samples(&mut self, out: &mut [f32]) {
        let mut buffer = self.buffer.lock().unwrap();
        if buffer.len() < out.len() {
            warn!("Not enough samples in buffer - needed {}, got {}", out.len(), buffer.len());
        }
        for x in out.iter_mut() {
            *x = buffer.pop_front().unwrap_or(0.0);
        }
    }

    pub fn write_samples(&mut self, samples: &[f32]) {
        let mut buffer = self.buffer.lock().unwrap();
        buffer.extend(samples);
    }

    pub fn clear(&mut self) {
        let mut buffer = self.buffer.lock().unwrap();
        buffer.clear();
    }
}

impl APU {
    pub fn new() -> APU {
        APU {
            output_buffer: None,

            square_wave1: SquareWave::new(),
            square_wave2: SquareWave::new(),
            triangle_wave: TriangleWave::new(),

            guest_enabled_channels: AudioChannels::empty(),
            host_enabled_channels: AudioChannels::all(),

            sq1_samples: Vec::new(),
            sq2_samples: Vec::new(),
            tri_samples: Vec::new(),
            mixed_samples: Vec::new(),

            last_cpu_cycles: 0,
        }
    }

    pub fn attach_output_device(&mut self, output_buffer: SampleBuffer) {
        self.output_buffer = Some(output_buffer);
    }

    pub fn run_until_cycle(&mut self, end_cpu_cycle: u64) {
        let start_cpu_cycle = self.last_cpu_cycles;
        // If we have no output, don't bother generating any samples
        let samples_per_second = self.output_buffer.as_ref().map(|b| b.samples_per_second).unwrap_or(0);

        let start_time_s = start_cpu_cycle as f64 / CPU_FREQ as f64;
        let step_duration_s = (end_cpu_cycle - start_cpu_cycle) as f64 / CPU_FREQ as f64;
        let samples_to_output = (samples_per_second as f64 * step_duration_s) as usize;

        self.sq1_samples.resize(samples_to_output, 0f32);
        self.sq2_samples.resize(samples_to_output, 0f32);
        self.tri_samples.resize(samples_to_output, 0f32);
        self.mixed_samples.resize(samples_to_output, 0f32);

        if self.channel_enabled(AudioChannels::SQUARE1) {
            self.square_wave1.output_samples(start_time_s, step_duration_s, &mut self.sq1_samples);
        }
        if self.channel_enabled(AudioChannels::SQUARE2) {
            self.square_wave2.output_samples(start_time_s, step_duration_s, &mut self.sq2_samples);
        }
        if self.channel_enabled(AudioChannels::TRIANGLE) {
            self.triangle_wave.output_samples(start_time_s, step_duration_s, &mut self.tri_samples);
        }

        for i in 0..samples_to_output {
            // Mixing formula from here: https://www.nesdev.org/wiki/APU_Mixer
            let pulse1 = self.sq1_samples[i];
            let pulse2 = self.sq2_samples[i];
            let triangle = self.tri_samples[i];
            let noise: f32 = 0.0;
            let dmc: f32 = 0.0;

            let pulse_out = 0.00752 * (pulse1 + pulse2);
            let tnd_out = 0.00851 * triangle + 0.00494 * noise + 0.00335 * dmc;
            let output = pulse_out + tnd_out;
            self.mixed_samples[i] = output;
        }

        if !self.mixed_samples.is_empty() {
            if let Some(output_buffer) = self.output_buffer.as_mut() {
                output_buffer.write_samples(&self.mixed_samples);
            }
        }

        self.last_cpu_cycles = end_cpu_cycle;
    }

    pub fn write_register(&mut self, addr: u16, value: u8, cpu_cycle: u64) {
        self.run_until_cycle(cpu_cycle);

        match addr {
            0x4000 => self.square_wave1.write_control(value),
            0x4001 => self.square_wave1.write_ramp(value),
            0x4002 => self.square_wave1.write_fine_tune(value),
            0x4003 => self.square_wave1.write_coarse_tune(value),

            0x4004 => self.square_wave2.write_control(value),
            0x4005 => self.square_wave2.write_ramp(value),
            0x4006 => self.square_wave2.write_fine_tune(value),
            0x4007 => self.square_wave2.write_coarse_tune(value),

            0x4008 => self.triangle_wave.write_control(value),
            0x400A => self.triangle_wave.write_fine_tune(value),
            0x400B => self.triangle_wave.write_coarse_tune(value),

            0x4015 => {
                self.guest_enabled_channels = AudioChannels::from_bits_truncate(value);
            }

            _ => {}
        }
    }

    fn channel_enabled(&self, channel: AudioChannels) -> bool {
        let enabled = self.host_enabled_channels & self.guest_enabled_channels;
        enabled.contains(channel)
    }

    pub fn toggle_channel(&mut self, channel: AudioChannels) {
        self.host_enabled_channels.toggle(channel);
        let state = if self.host_enabled_channels.contains(channel) { "on" } else { "off" };
        info!("Toggled channel {channel:?} to {state}")
    }
}

const CPU_FREQ: u32 = 1_789_773; // 1.789773 MHz

struct SquareWave {
    volume: f32,

    duty_cycle: f32,
    period: u32,
}

impl SquareWave {
    fn new() -> SquareWave {
        SquareWave {
            volume: 1.0,
            duty_cycle: 0.5,
            period: 0, // Range: 0-0x7FF / 0-2047 / 12.428KHz-54Hz
        }
    }

    fn output_samples(
        &mut self,
        step_start_time_s: f64,
        step_duration_s: f64,
        output: &mut [f32],
    ) {
        if self.period < 8 {
            output.fill(0.0);
            // All zeroes
            return;
        }

        let period_s: f64 = (16 * (self.period + 1)) as f64 / CPU_FREQ as f64;
        let time_step = step_duration_s / output.len() as f64;
        for (i, sample) in output.iter_mut().enumerate() {
            let now_s = step_start_time_s + time_step * i as f64;
            let phase = (now_s / period_s) % 1.0;
            if phase <= self.duty_cycle as f64 { // duty_cycle
                *sample = self.volume;
            } else {
                *sample = -self.volume;
            };
        }
    }

    // $4003/$4007
    fn write_coarse_tune(&mut self, value: u8) {
        // TODO: Reset the phase
        self.period = self.period & 0x00FF | ((value as u32 & 0x7) << 8);
        // TODO: Reset length counter
    }

    // $4002/$4006
    fn write_fine_tune(&mut self, value: u8) {
        self.period = self.period & 0xFF00 | value as u32;
    }

    // $4000/$4004
    fn write_control(&mut self, value: u8) {
        self.duty_cycle = match value >> 6 {
            0 => 0.125,
            1 => 0.25,
            2 => 0.5,
            3 => 0.75,
            _ => unreachable!(),
        };
    }

    // $4001/$4005
    fn write_ramp(&mut self, _value: u8) {

    }
}

struct TriangleWave {
    period: u32,
}

impl TriangleWave {
    fn new() -> TriangleWave {
        TriangleWave {
            period: 0,
        }
    }

    fn output_samples(
        &mut self,
        step_start_time_s: f64,
        step_duration_s: f64,
        output: &mut [f32],
    ) {
        if self.period < 2 {
            output.fill(0.0);
            // All zeroes
            return;
        }

        let period_s: f64 = (32 * (self.period + 1)) as f64 / CPU_FREQ as f64;
        let time_step = step_duration_s / output.len() as f64;
        for (i, sample) in output.iter_mut().enumerate() {
            let now_s = step_start_time_s + time_step * i as f64;
            let scaled: f64  = now_s / period_s * 4.0;
            // Number between 0 and 3 - which of the 4 sections of the triangle wave are we in
            let cycle_phase = scaled as i64 % 4;
            // Number between 0 and 1 - how far through a single section are we
            let cycle_offset = (scaled % 1.0) as f32;

            // TODO: Quantize into 4-bit values
            *sample = match cycle_phase {
                0 => cycle_offset, // 0 to 1
                1 => 1.0 - cycle_offset, // 1 to 0
                2 => -cycle_offset, // 0 to -1
                3 => -1.0 + cycle_offset, // -1 to 0
                _ => unreachable!(),
            };
        }
    }

    // $4008
    fn write_control(&mut self, _value: u8) {

    }

    // $400A
    fn write_fine_tune(&mut self, value: u8) {
        self.period = self.period & 0xFF00 | (value as u32);
    }

    // $400B
    fn write_coarse_tune(&mut self, value: u8) {
        self.period = self.period & 0x00FF | ((value as u32 & 0x7) << 8);
    }
}
