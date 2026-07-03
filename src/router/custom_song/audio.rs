use std::num::{NonZeroU8, NonZeroU32};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::TrackType;
use symphonia::core::formats::probe::Hint;
use symphonia::core::io::MediaSourceStream;
use vorbis_rs::{VorbisBitrateManagementStrategy, VorbisEncoderBuilder};

use super::{DEFAULT_PREVIEW_LENGTH_SEC, PREVIEW_FADE_SEC};

// The whole audio pipeline runs in-process: symphonia (pure Rust) decodes and
// validates uploads, vorbis_rs (libvorbis compiled into the binary - a library
// call, like rusqlite's bundled sqlite) encodes. No external processes are
// ever spawned and nothing needs to be on PATH.
//
// - ogg-vorbis uploads are stored AS-IS once they prove decodable, so the
//   served play cue's md5 is stable across servers (export/import keeps it)
// - mp3/wav uploads are transcoded to ogg-vorbis once, at upload time
// - the select cue (menu preview) is cut + faded in samples and re-encoded
// Encodes keep the source sample rate and use a fixed Ogg stream serial, so
// they're deterministic: the same input always produces the same bytes

pub struct Cue {
    pub bytes: Vec<u8>,
    pub md5: String,
    pub duration_sec: f64
}

// ~ffmpeg's -q:a 6
const ENCODE_QUALITY: f32 = 0.6;
// "SIF2" - fixed so encoding is deterministic
const STREAM_SERIAL: i32 = 0x53494632;
const ENCODE_BLOCK_FRAMES: usize = 65536;

struct DecodedAudio {
    // Planar f32, one Vec per channel. Channels past the first two are dropped
    channels: Vec<Vec<f32>>,
    sample_rate: u32
}

impl DecodedAudio {
    fn frames(&self) -> usize {
        self.channels[0].len()
    }
    fn duration(&self) -> f64 {
        self.frames() as f64 / self.sample_rate as f64
    }
}

fn is_ogg_vorbis(bytes: &[u8]) -> bool {
    // "OggS" capture pattern + the "\x01vorbis" identification header on the first page
    bytes.starts_with(b"OggS") && bytes.len() > 64 && bytes[..64].windows(7).any(|w| w == b"\x01vorbis")
}

fn decode(bytes: &[u8]) -> Result<DecodedAudio, String> {
    let stream = MediaSourceStream::new(Box::new(std::io::Cursor::new(bytes.to_vec())), Default::default());
    let mut format = symphonia::default::get_probe()
        .probe(&Hint::new(), stream, Default::default(), Default::default())
        .map_err(|_| String::from("Could not read audio file (expected ogg vorbis, mp3 or wav)"))?;

    let track = format.default_track(TrackType::Audio).ok_or(String::from("Audio file has no audio track"))?;
    let track_id = track.id;
    let Some(CodecParameters::Audio(params)) = track.codec_params.clone() else {
        return Err(String::from("Audio file has no audio track"));
    };
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|_| String::from("Could not read audio file (expected ogg vorbis, mp3 or wav)"))?;

    let mut channels: Vec<Vec<f32>> = Vec::new();
    let mut sample_rate = 0;
    let mut interleaved: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(SymphoniaError::ResetRequired) => break,
            Err(_) => return Err(String::from("Audio file is corrupt or truncated"))
        };
        if packet.track_id != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            // Decoders treat a bad packet as recoverable; skip it like they do
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(_) => return Err(String::from("Audio file is corrupt or truncated"))
        };
        let count = decoded.spec().channels().count();
        if channels.is_empty() {
            sample_rate = decoded.spec().rate();
            channels = vec![Vec::new(); std::cmp::min(count, 2)];
        }
        decoded.copy_to_vec_interleaved(&mut interleaved);
        for (i, samples) in channels.iter_mut().enumerate() {
            samples.extend(interleaved.iter().skip(i).step_by(count));
        }
    }

    if channels.is_empty() || channels[0].is_empty() || sample_rate == 0 {
        return Err(String::from("Audio file is corrupt or truncated"));
    }
    Ok(DecodedAudio { channels, sample_rate })
}

fn encode(channels: &[&[f32]], sample_rate: u32) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut builder = VorbisEncoderBuilder::new_with_serial(
        NonZeroU32::new(sample_rate).ok_or(String::from("Audio file is corrupt or truncated"))?,
        NonZeroU8::new(channels.len() as u8).unwrap(),
        &mut out,
        STREAM_SERIAL
    );
    builder.bitrate_management_strategy(VorbisBitrateManagementStrategy::QualityVbr {
        target_quality: ENCODE_QUALITY
    });
    let mut encoder = builder.build().map_err(|e| format!("Audio encode failed: {}", e))?;

    let frames = channels[0].len();
    let mut i = 0;
    while i < frames {
        let end = std::cmp::min(i + ENCODE_BLOCK_FRAMES, frames);
        let block: Vec<&[f32]> = channels.iter().map(|samples| &samples[i..end]).collect();
        encoder.encode_audio_block(&block).map_err(|e| format!("Audio encode failed: {}", e))?;
        i = end;
    }
    encoder.finish().map_err(|e| format!("Audio encode failed: {}", e))?;
    Ok(out)
}

fn cue(bytes: Vec<u8>, duration_sec: f64) -> Cue {
    Cue {
        md5: format!("{:x}", md5::compute(&bytes)),
        bytes,
        duration_sec
    }
}

// The play cue is the full track, the select cue is a preview cut with short
// fades. Both are stored content-addressed by the md5 of the final ogg bytes -
// the client validates md5(file) against the value served in the catalog
pub fn process(bytes: &[u8], preview_start_sec: Option<f64>, preview_length_sec: Option<f64>) -> Result<(Cue, Cue), String> {
    let audio = decode(bytes)?;
    let duration = audio.duration();
    if duration <= 1.0 {
        return Err(String::from("Audio track is too short"));
    }

    let planar: Vec<&[f32]> = audio.channels.iter().map(|samples| samples.as_slice()).collect();
    let play = if is_ogg_vorbis(bytes) {
        // Already ogg-vorbis and proven decodable: keep the exact bytes
        cue(bytes.to_vec(), duration)
    } else {
        cue(encode(&planar, audio.sample_rate)?, duration)
    };

    // Preview defaults: start 30% into the track, 30 seconds long
    let mut start = preview_start_sec.unwrap_or(duration * 0.3);
    if start < 0.0 || start >= duration {
        start = duration * 0.3;
    }
    let length = preview_length_sec.unwrap_or(DEFAULT_PREVIEW_LENGTH_SEC).clamp(1.0, duration - start);
    let start_frame = (start * audio.sample_rate as f64) as usize;
    let end_frame = std::cmp::min(start_frame + (length * audio.sample_rate as f64) as usize, audio.frames());

    let fade_frames = (PREVIEW_FADE_SEC * audio.sample_rate as f64) as usize;
    let mut segment: Vec<Vec<f32>> = audio.channels.iter().map(|samples| samples[start_frame..end_frame].to_vec()).collect();
    let frames = end_frame - start_frame;
    if frames > fade_frames * 2 {
        for samples in segment.iter_mut() {
            for i in 0..fade_frames {
                let gain = i as f32 / fade_frames as f32;
                samples[i] *= gain;
                samples[frames - 1 - i] *= gain;
            }
        }
    }
    let planar: Vec<&[f32]> = segment.iter().map(|samples| samples.as_slice()).collect();
    let select = cue(encode(&planar, audio.sample_rate)?, frames as f64 / audio.sample_rate as f64);

    Ok((play, select))
}
