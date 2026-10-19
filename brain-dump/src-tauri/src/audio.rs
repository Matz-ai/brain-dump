use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use hound::{WavSpec, WavWriter};
use std::sync::{Arc, Mutex};
use std::path::PathBuf;

#[derive(Debug, Clone, serde::Serialize)]
pub struct MicDevice {
    pub name: String,
    pub is_default: bool,
}

pub fn list_microphones() -> Vec<MicDevice> {
    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .and_then(|d| d.name().ok())
        .unwrap_or_default();

    let mut devices = Vec::new();
    if let Ok(input_devices) = host.input_devices() {
        for device in input_devices {
            if let Ok(name) = device.name() {
                devices.push(MicDevice {
                    is_default: name == default_name,
                    name,
                });
            }
        }
    }
    devices
}

/// Wrapper to make cpal::Stream usable across threads.
/// SAFETY: cpal::Stream on macOS (CoreAudio) is thread-safe in practice;
/// we only access it behind a Mutex to start/stop recording.
struct SendStream(#[allow(dead_code)] cpal::Stream);
unsafe impl Send for SendStream {}
unsafe impl Sync for SendStream {}

pub struct AudioRecorder {
    samples: Arc<Mutex<Vec<f32>>>,
    stream: Option<SendStream>,
    source_sample_rate: u32,
    source_channels: u16,
}

impl AudioRecorder {
    pub fn new() -> Self {
        Self {
            samples: Arc::new(Mutex::new(Vec::new())),
            stream: None,
            source_sample_rate: 48000,
            source_channels: 1,
        }
    }

    pub fn start(&mut self, mic_name: &str) -> Result<(), String> {
        // Clear any leftover samples from previous recording
        self.samples.lock().unwrap().clear();

        let host = cpal::default_host();

        let device = if mic_name == "default" {
            host.default_input_device()
                .ok_or("No default input device found")?
        } else {
            host.input_devices()
                .map_err(|e| e.to_string())?
                .find(|d| d.name().map(|n| n == mic_name).unwrap_or(false))
                .ok_or(format!("Microphone '{}' not found", mic_name))?
        };

        // Use the device's default config instead of forcing 16kHz
        let default_config = device
            .default_input_config()
            .map_err(|e| format!("Failed to get default input config: {}", e))?;

        let sample_rate = default_config.sample_rate().0;
        let channels = default_config.channels();

        println!("[Typr] Mic config: {}Hz, {} channels", sample_rate, channels);

        self.source_sample_rate = sample_rate;
        self.source_channels = channels;

        let config = cpal::StreamConfig {
            channels,
            sample_rate: cpal::SampleRate(sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        let samples = self.samples.clone();
        let stream = device
            .build_input_stream(
                &config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let mut buf = samples.lock().unwrap();
                    buf.extend_from_slice(data);
                },
                |err| {
                    eprintln!("[Typr] Audio stream error: {}", err);
                },
                None,
            )
            .map_err(|e| e.to_string())?;

        stream.play().map_err(|e| e.to_string())?;
        self.stream = Some(SendStream(stream));
        println!("[Typr] Audio recording started");
        Ok(())
    }

    /// Loudness of the last ~50 ms captured, from 0 (silence) to 1 (loud speech), for the overlay meter.
    pub fn level(&self) -> f32 {
        let samples = self.samples.lock().unwrap();
        let window = (self.source_sample_rate as usize * self.source_channels as usize) / 20;
        let tail = &samples[samples.len().saturating_sub(window)..];
        if tail.is_empty() {
            return 0.0;
        }
        let rms = (tail.iter().map(|x| x * x).sum::<f32>() / tail.len() as f32).sqrt();
        level_from_rms(rms)
    }

    /// Stops the stream and writes a 16kHz mono WAV with leading/trailing silence trimmed.
    /// Returns Ok(false) (and writes nothing) when no speech was detected: sending silence
    /// to Whisper is what produces "Sous-titres Radio-Canada"-style hallucinations.
    pub fn stop_and_save(&mut self, output_path: &PathBuf) -> Result<bool, String> {
        self.stream = None; // Drop stops the stream
        println!("[Typr] Audio recording stopped");

        let samples = self.samples.lock().unwrap();
        if samples.is_empty() {
            return Err("No audio captured".to_string());
        }

        println!("[Typr] Captured {} raw samples", samples.len());

        // Convert to mono if multi-channel
        let mono: Vec<f32> = if self.source_channels > 1 {
            samples
                .chunks(self.source_channels as usize)
                .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
                .collect()
        } else {
            samples.clone()
        };

        // Downsample to 16kHz for whisper.cpp
        let resampled = resample(&mono, self.source_sample_rate, 16000);
        println!("[Typr] Resampled to {} samples at 16kHz", resampled.len());

        let speech = match trim_silence(&resampled, 16000) {
            Some(s) => normalize(s),
            None => {
                println!("[Typr] No speech detected, skipping transcription");
                drop(samples);
                self.samples.lock().unwrap().clear();
                return Ok(false);
            }
        };

        let spec = WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };

        let mut writer = WavWriter::create(output_path, spec).map_err(|e| e.to_string())?;
        for &sample in speech.iter() {
            let amplitude = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            writer.write_sample(amplitude).map_err(|e| e.to_string())?;
        }
        writer.finalize().map_err(|e| e.to_string())?;

        drop(samples);
        self.samples.lock().unwrap().clear();

        println!("[Typr] WAV saved to {:?}", output_path);
        Ok(true)
    }
}

/// Maps RMS to 0..1 on a dB scale: -50 dBFS (room noise) → 0, -12 dBFS (loud voice) → 1.
fn level_from_rms(rms: f32) -> f32 {
    const FLOOR_DB: f32 = -50.0;
    const CEIL_DB: f32 = -12.0;
    let db = 20.0 * rms.max(1e-6).log10();
    ((db - FLOOR_DB) / (CEIL_DB - FLOOR_DB)).clamp(0.0, 1.0)
}

const FRAME_MS: usize = 20;
/// Minimum RMS (~ -46 dBFS) for a frame to count as speech, whatever the noise floor.
const MIN_SPEECH_RMS: f32 = 0.005;
/// Below this much detected speech, the recording is treated as empty.
const MIN_SPEECH_MS: usize = 200;
const PAD_BEFORE_MS: usize = 300;
const PAD_AFTER_MS: usize = 500;

/// Returns the slice between the first and last speech frames (with padding),
/// or None if the recording contains no speech.
fn trim_silence(samples: &[f32], sample_rate: usize) -> Option<&[f32]> {
    let frame_len = sample_rate * FRAME_MS / 1000;
    let rms: Vec<f32> = samples
        .chunks(frame_len)
        .map(|f| (f.iter().map(|x| x * x).sum::<f32>() / f.len() as f32).sqrt())
        .collect();
    if rms.is_empty() {
        return None;
    }

    // Adaptive threshold: well above the noise floor (10th percentile frame).
    let mut sorted = rms.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let noise_floor = sorted[sorted.len() / 10];
    let threshold = (noise_floor * 3.0).max(MIN_SPEECH_RMS);

    let speech_frames = rms.iter().filter(|&&r| r > threshold).count();
    if speech_frames * FRAME_MS < MIN_SPEECH_MS {
        return None;
    }

    let first = rms.iter().position(|&r| r > threshold)?;
    let last = rms.iter().rposition(|&r| r > threshold)?;
    let start = (first * frame_len).saturating_sub(sample_rate * PAD_BEFORE_MS / 1000);
    let end = ((last + 1) * frame_len + sample_rate * PAD_AFTER_MS / 1000).min(samples.len());
    Some(&samples[start..end])
}

/// Peak-normalizes quiet recordings (gain capped at 10x) so Whisper hears them clearly.
fn normalize(samples: &[f32]) -> Vec<f32> {
    let peak = samples.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    let gain = if peak > 0.0 { (0.9 / peak).clamp(1.0, 10.0) } else { 1.0 };
    samples.iter().map(|x| x * gain).collect()
}

/// Simple linear interpolation resampler
fn resample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate {
        return samples.to_vec();
    }

    let ratio = from_rate as f64 / to_rate as f64;
    let output_len = (samples.len() as f64 / ratio) as usize;
    let mut output = Vec::with_capacity(output_len);

    for i in 0..output_len {
        let src_idx = i as f64 * ratio;
        let idx = src_idx as usize;
        let frac = src_idx - idx as f64;

        let sample = if idx + 1 < samples.len() {
            samples[idx] as f64 * (1.0 - frac) + samples[idx + 1] as f64 * frac
        } else {
            samples[idx.min(samples.len() - 1)] as f64
        };

        output.push(sample as f32);
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: usize = 16000;

    #[test]
    fn test_level_from_rms() {
        assert_eq!(level_from_rms(0.0), 0.0);
        assert_eq!(level_from_rms(0.001), 0.0); // -60 dBFS : bruit de fond
        assert_eq!(level_from_rms(0.5), 1.0); // -6 dBFS : plafonné
        let voice = level_from_rms(0.05); // -26 dBFS : voix normale
        assert!(voice > 0.5 && voice < 0.8, "{voice}");
    }

    fn tone(ms: usize, amp: f32) -> Vec<f32> {
        (0..SR * ms / 1000)
            .map(|i| amp * (i as f32 * 0.05).sin())
            .collect()
    }

    #[test]
    fn test_silence_is_rejected() {
        assert!(trim_silence(&vec![0.0; SR * 2], SR).is_none());
        // Low background noise only
        assert!(trim_silence(&tone(2000, 0.003), SR).is_none());
    }

    #[test]
    fn test_click_is_rejected() {
        // A 60ms key click in 2s of silence is not speech
        let mut s = vec![0.0; SR];
        s.extend(tone(60, 0.5));
        s.extend(vec![0.0; SR]);
        assert!(trim_silence(&s, SR).is_none());
    }

    #[test]
    fn test_speech_is_trimmed_with_padding() {
        let mut s = vec![0.0; SR * 2]; // 2s silence
        s.extend(tone(1000, 0.2)); // 1s speech
        s.extend(vec![0.0; SR * 3]); // 3s silence
        let trimmed = trim_silence(&s, SR).unwrap();
        let ms = trimmed.len() * 1000 / SR;
        assert!(ms >= 1000 && ms <= 1000 + PAD_BEFORE_MS + PAD_AFTER_MS + FRAME_MS * 2, "{ms}");
    }

    #[test]
    fn test_normalize_boosts_quiet_audio_with_cap() {
        let quiet = normalize(&[0.01, -0.01]);
        assert!((quiet[0] - 0.1).abs() < 1e-6); // capped at 10x
        let loud = normalize(&[0.9, -0.5]);
        assert!((loud[0] - 0.9).abs() < 1e-6); // never attenuated
    }
}
