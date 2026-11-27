pub const ESP32_OUT_SAMPLE_RATE: u32 = 22_050;
pub const MAX_WAV_BYTES: usize = 64_800;

/// Decodes a WAV byte slice into a 16kHz mono `Vec<f32>` stream.
/// Validates RIFF/WAVE header and properly iterates RIFF chunks (ignoring JUNK, LIST, etc.).
pub fn decode_wav_to_16k_mono(wav_bytes: &[u8]) -> Result<Vec<f32>, String> {
    if wav_bytes.len() < 44 {
        return Err("WAV file too short (< 44 bytes)".to_string());
    }

    if &wav_bytes[0..4] != b"RIFF" || &wav_bytes[8..12] != b"WAVE" {
        return Err("Invalid RIFF/WAVE header".to_string());
    }

    let mut cursor = 12;
    let mut fmt_found = false;
    let mut audio_format = 0u16;
    let mut channels = 0u16;
    let mut sample_rate = 0u32;
    let mut bits_per_sample = 0u16;

    let mut data_bytes: &[u8] = &[];

    while cursor + 8 <= wav_bytes.len() {
        let chunk_id = &wav_bytes[cursor..cursor + 4];
        let chunk_size = u32::from_le_bytes(
            wav_bytes[cursor + 4..cursor + 8]
                .try_into()
                .map_err(|_| "Failed to read chunk size")?,
        ) as usize;
        cursor += 8;

        if cursor + chunk_size > wav_bytes.len() {
            // Truncated chunk, clamp or break
            break;
        }

        let chunk_data = &wav_bytes[cursor..cursor + chunk_size];
        cursor += chunk_size;
        // Chunks are word-aligned (padded with a dummy byte if size is odd)
        if chunk_size % 2 != 0 && cursor < wav_bytes.len() {
            cursor += 1;
        }

        if chunk_id == b"fmt " {
            if chunk_data.len() < 16 {
                return Err("Invalid 'fmt ' chunk size".to_string());
            }
            audio_format = u16::from_le_bytes(chunk_data[0..2].try_into().unwrap());
            channels = u16::from_le_bytes(chunk_data[2..4].try_into().unwrap());
            sample_rate = u32::from_le_bytes(chunk_data[4..8].try_into().unwrap());
            bits_per_sample = u16::from_le_bytes(chunk_data[14..16].try_into().unwrap());
            fmt_found = true;
        } else if chunk_id == b"data" {
            data_bytes = chunk_data;
        }
    }

    if !fmt_found {
        return Err("Missing 'fmt ' chunk in WAV".to_string());
    }
    if data_bytes.is_empty() {
        return Err("Missing 'data' chunk in WAV".to_string());
    }

    if audio_format != 1 {
        return Err(format!("Unsupported audio format: {} (expected 1 for PCM)", audio_format));
    }
    if bits_per_sample != 16 {
        return Err(format!("Unsupported bits per sample: {} (expected 16)", bits_per_sample));
    }
    if channels != 1 && channels != 2 {
        return Err(format!("Unsupported channels: {} (expected 1 or 2)", channels));
    }
    if !(8_000..=48_000).contains(&sample_rate) {
        return Err(format!("Unsupported sample rate: {}", sample_rate));
    }

    // Convert PCM16 LE to f32, mix stereo to mono if needed
    let num_samples = data_bytes.len() / 2;
    let mut mono_f32 = Vec::with_capacity(num_samples / (channels as usize));

    let mut i = 0;
    while i + 1 < data_bytes.len() {
        if channels == 2 {
            if i + 3 < data_bytes.len() {
                let left_raw = i16::from_le_bytes([data_bytes[i], data_bytes[i + 1]]);
                let right_raw = i16::from_le_bytes([data_bytes[i + 2], data_bytes[i + 3]]);
                let l = left_raw as f32 / 32768.0;
                let r = right_raw as f32 / 32768.0;
                mono_f32.push((l + r) * 0.5);
                i += 4;
            } else {
                break;
            }
        } else {
            let sample_raw = i16::from_le_bytes([data_bytes[i], data_bytes[i + 1]]);
            mono_f32.push(sample_raw as f32 / 32768.0);
            i += 2;
        }
    }

    // Resample to 16,000 Hz if necessary
    if sample_rate != 16_000 {
        let mut resampler = crate::speech::audio::Resampler::new(sample_rate);
        let mut out = Vec::new();
        resampler.process(&mono_f32, &mut out);
        Ok(out)
    } else {
        Ok(mono_f32)
    }
}

/// Packs canonical 22,050 Hz mono WAV bytes (exactly 44 bytes header) from `i16` samples.
pub fn pack_canonical_22050_wav(samples_22050: &[i16], max_wav_bytes: usize) -> Vec<u8> {
    let mut samples = samples_22050.to_vec();
    let header_len = 44;
    let max_data_bytes = if max_wav_bytes > header_len {
        max_wav_bytes - header_len
    } else {
        0
    };
    let max_samples = max_data_bytes / 2;

    if samples.len() > max_samples {
        samples.truncate(max_samples);
        // Apply linear fade-out on the last 441 samples (~20 ms at 22.05kHz) if possible
        let fade_len = std::cmp::min(441, samples.len());
        let start_idx = samples.len() - fade_len;
        for (idx, sample) in samples[start_idx..].iter_mut().enumerate() {
            let factor = (fade_len - idx) as f32 / fade_len as f32;
            *sample = (*sample as f32 * factor) as i16;
        }
    }

    let data_len = (samples.len() * 2) as u32;
    let file_len = data_len + 36;

    let mut wav = Vec::with_capacity(header_len + samples.len() * 2);

    // RIFF header
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&file_len.to_le_bytes());
    wav.extend_from_slice(b"WAVE");

    // fmt chunk
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // Subchunk1Size (16 for PCM)
    wav.extend_from_slice(&1u16.to_le_bytes());  // AudioFormat (1 = PCM)
    wav.extend_from_slice(&1u16.to_le_bytes());  // NumChannels (1 = Mono)
    wav.extend_from_slice(&ESP32_OUT_SAMPLE_RATE.to_le_bytes()); // SampleRate (22050)
    let byte_rate = ESP32_OUT_SAMPLE_RATE * 1 * 2;
    wav.extend_from_slice(&byte_rate.to_le_bytes()); // ByteRate
    wav.extend_from_slice(&2u16.to_le_bytes());  // BlockAlign (1 channel * 2 bytes)
    wav.extend_from_slice(&16u16.to_le_bytes()); // BitsPerSample (16)

    // data chunk
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());

    for &s in &samples {
        wav.extend_from_slice(&s.to_le_bytes());
    }

    wav
}

/// Returns a canonical silence WAV of 44 bytes header + PCM16 samples for the given duration in ms at 22,050 Hz.
pub fn fallback_silence_wav_22050(duration_ms: u32) -> Vec<u8> {
    let num_samples = ((ESP32_OUT_SAMPLE_RATE as u64 * duration_ms as u64) / 1000) as usize;
    let samples = vec![0i16; num_samples];
    pack_canonical_22050_wav(&samples, MAX_WAV_BYTES)
}

/// Synthesizes text to 22,050 Hz WAV bytes via Windows Media SpeechSynthesis API on Windows,
/// or returns silence fallback otherwise.
pub fn synthesize_tts_wav_22050(text: &str) -> Vec<u8> {
    #[cfg(all(windows, not(test)))]
    {
        match synthesize_windows_rt(text) {
            Ok(bytes) => bytes,
            Err(_) => fallback_silence_wav_22050(120),
        }
    }

    #[cfg(not(all(windows, not(test))))]
    {
        let _ = text;
        fallback_silence_wav_22050(120)
    }
}

#[cfg(all(windows, not(test)))]
fn synthesize_windows_rt(text: &str) -> Result<Vec<u8>, String> {
    use windows::Media::SpeechSynthesis::SpeechSynthesizer;
    use windows::Storage::Streams::DataReader;

    let synthesizer = SpeechSynthesizer::new().map_err(|e| e.to_string())?;
    let htext = windows::core::HSTRING::from(text);
    let stream = synthesizer
        .SynthesizeTextToStreamAsync(&htext)
        .and_then(|op| op.get())
        .map_err(|e| e.to_string())?;
    let size = stream.Size().map_err(|e| e.to_string())? as usize;

    let input_stream = stream.GetInputStreamAt(0).map_err(|e| e.to_string())?;
    let reader = DataReader::CreateDataReader(&input_stream).map_err(|e| e.to_string())?;
    reader
        .LoadAsync(size as u32)
        .and_then(|op| op.get())
        .map_err(|e| e.to_string())?;

    let mut buf = vec![0u8; size];
    reader.ReadBytes(&mut buf).map_err(|e| e.to_string())?;

    // Decode the WAV returned by Windows (which might be PCM or have standard headers, skip/parse chunks)
    let decoded_f32 = match decode_wav_to_any_rate(&buf) {
        Ok((samples, sample_rate)) => {
            // Resample to 22050 Hz if needed
            if sample_rate != ESP32_OUT_SAMPLE_RATE {
                resample_linear(&samples, sample_rate, ESP32_OUT_SAMPLE_RATE)
            } else {
                samples
            }
        }
        Err(_) => {
            // If decoding fails, return fallback
            return Ok(fallback_silence_wav_22050(120));
        }
    };

    // Convert f32 samples back to i16
    let i16_samples: Vec<i16> = decoded_f32
        .into_iter()
        .map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect();

    Ok(pack_canonical_22050_wav(&i16_samples, MAX_WAV_BYTES))
}

#[cfg(all(windows, not(test)))]
fn decode_wav_to_any_rate(wav_bytes: &[u8]) -> Result<(Vec<f32>, u32), String> {
    if wav_bytes.len() < 44 {
        return Err("WAV too short".into());
    }
    // Simple helper to extract sample rate and samples
    let mut cursor = 12;
    let mut sample_rate = 16_000u32;
    let mut data_bytes: &[u8] = &[];

    while cursor + 8 <= wav_bytes.len() {
        let chunk_id = &wav_bytes[cursor..cursor + 4];
        let chunk_size = u32::from_le_bytes(wav_bytes[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
        cursor += 8;
        if cursor + chunk_size > wav_bytes.len() {
            break;
        }
        let chunk_data = &wav_bytes[cursor..cursor + chunk_size];
        cursor += chunk_size;
        if chunk_size % 2 != 0 && cursor < wav_bytes.len() {
            cursor += 1;
        }

        if chunk_id == b"fmt " && chunk_data.len() >= 8 {
            sample_rate = u32::from_le_bytes(chunk_data[4..8].try_into().unwrap());
        } else if chunk_id == b"data" {
            data_bytes = chunk_data;
        }
    }

    if data_bytes.is_empty() {
        return Err("No data chunk".into());
    }

    let mut samples = Vec::with_capacity(data_bytes.len() / 2);
    let mut i = 0;
    while i + 1 < data_bytes.len() {
        let s = i16::from_le_bytes([data_bytes[i], data_bytes[i + 1]]);
        samples.push(s as f32 / 32768.0);
        i += 2;
    }

    Ok((samples, sample_rate))
}

#[cfg(all(windows, not(test)))]
fn resample_linear(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if input.is_empty() || from_rate == to_rate {
        return input.to_vec();
    }
    let ratio = from_rate as f64 / to_rate as f64;
    let output_len = (input.len() as f64 / ratio) as usize;
    let mut output = Vec::with_capacity(output_len);

    for i in 0..output_len {
        let src_idx = i as f64 * ratio;
        let idx_lo = src_idx.floor() as usize;
        let idx_hi = (idx_lo + 1).min(input.len() - 1);
        let frac = (src_idx - idx_lo as f64) as f32;

        let val = input[idx_lo] * (1.0 - frac) + input[idx_hi] * frac;
        output.push(val);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_wav(sample_rate: u32, channels: u16, pcm_data: &[u8], extra_chunks: &[(&[u8], &[u8])]) -> Vec<u8> {
        let mut data = Vec::new();
        // Calculate total extra chunks size
        let mut extra_size = 0;
        for (id, content) in extra_chunks {
            extra_size += 8 + content.len() + (content.len() % 2);
            let _ = id;
        }

        let fmt_size = 16u32;
        let data_size = pcm_data.len() as u32;
        let file_size = 4 + 8 + fmt_size + 8 + (extra_size as u32) + 8 + data_size;

        data.extend_from_slice(b"RIFF");
        data.extend_from_slice(&file_size.to_le_bytes());
        data.extend_from_slice(b"WAVE");

        // fmt chunk
        data.extend_from_slice(b"fmt ");
        data.extend_from_slice(&fmt_size.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes()); // PCM
        data.extend_from_slice(&channels.to_le_bytes());
        data.extend_from_slice(&sample_rate.to_le_bytes());
        let byte_rate = sample_rate * channels as u32 * 2;
        data.extend_from_slice(&byte_rate.to_le_bytes());
        let block_align = channels * 2;
        data.extend_from_slice(&block_align.to_le_bytes());
        data.extend_from_slice(&16u16.to_le_bytes()); // bits per sample

        // Extra chunks (e.g. JUNK, LIST)
        for (id, content) in extra_chunks {
            data.extend_from_slice(*id);
            data.extend_from_slice(&(content.len() as u32).to_le_bytes());
            data.extend_from_slice(*content);
            if content.len() % 2 != 0 {
                data.push(0);
            }
        }

        // data chunk
        data.extend_from_slice(b"data");
        data.extend_from_slice(&data_size.to_le_bytes());
        data.extend_from_slice(pcm_data);

        data
    }

    #[test]
    fn test_decode_wav_with_junk_and_list() {
        // 2 samples of silence in PCM16 mono at 16000Hz
        let pcm = vec![0u8; 4];
        let wav = create_test_wav(
            16_000,
            1,
            &pcm,
            &[
                (b"JUNK", b"some junk data here"),
                (b"LIST", b"some list info"),
            ],
        );

        let decoded = decode_wav_to_16k_mono(&wav);
        assert!(decoded.is_ok());
        let samples = decoded.unwrap();
        assert_eq!(samples.len(), 2);
    }

    #[test]
    fn test_decode_stereo_48k_to_16k() {
        // 480 samples per channel (10ms at 48kHz -> ~160 samples at 16kHz)
        let pcm = vec![0u8; 480 * 4];
        let wav = create_test_wav(48_000, 2, &pcm, &[]);

        let decoded = decode_wav_to_16k_mono(&wav);
        assert!(decoded.is_ok());
        let samples = decoded.unwrap();
        assert!(!samples.is_empty());
    }

    #[test]
    fn test_pack_canonical_22050_wav() {
        let samples = vec![1000i16; 50_000]; // enough to exceed MAX_WAV_BYTES
        let packed = pack_canonical_22050_wav(&samples, MAX_WAV_BYTES);

        assert!(packed.len() <= MAX_WAV_BYTES);
        assert_eq!(&packed[0..4], b"RIFF");
        assert_eq!(&packed[8..12], b"WAVE");
        // Sample rate at bytes 24..28 should be 22050
        let sr = u32::from_le_bytes(packed[24..28].try_into().unwrap());
        assert_eq!(sr, 22_050);
    }

    #[test]
    fn test_fallback_silence_wav_22050() {
        let silence = fallback_silence_wav_22050(120);
        assert!(silence.len() > 1000);
        assert_eq!(&silence[0..4], b"RIFF");
        let sr = u32::from_le_bytes(silence[24..28].try_into().unwrap());
        assert_eq!(sr, 22_050);
    }

    #[test]
    fn test_synthesize_tts_wav_22050_fallback() {
        let res = synthesize_tts_wav_22050("oi");
        assert!(res.len() > 1000);
        assert_eq!(&res[0..4], b"RIFF");
    }
}
