//! Rendering without an audio device, for checks.

use anyhow::Result;
use onezan_vst3_host::{MidiEvent, Plugin};

/// Renders `frames` frames of `plugin` offline, applying each `(frame, event)` at its frame.
/// For checks without an audio device.
///
/// # Errors
///
/// When the plugin cannot start.
pub fn render_offline(
    plugin: &mut Plugin,
    sample_rate: u32,
    frames: usize,
    mut events: Vec<(usize, MidiEvent)>,
) -> Result<(Vec<f32>, Vec<f32>)> {
    const BLOCK: usize = 256;
    events.sort_by_key(|(at, _)| *at);
    let mut p = plugin.start(f64::from(sample_rate), BLOCK)?;
    let (mut left, mut right) = (vec![0.0; frames], vec![0.0; frames]);
    let mut pending = events.into_iter().peekable();
    let mut block_events = Vec::new();
    for (i, (l, r)) in left.chunks_mut(BLOCK).zip(right.chunks_mut(BLOCK)).enumerate() {
        let start = i.saturating_mul(BLOCK);
        let end = start.saturating_add(BLOCK);
        block_events.clear();
        while let Some((at, ev)) = pending.next_if(|(at, _)| *at < end) {
            block_events.push((at.saturating_sub(start), ev));
        }
        p.process(&block_events, l, r);
    }
    drop(p);
    plugin.stop();
    Ok((left, right))
}
