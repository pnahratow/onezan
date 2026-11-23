/// A MIDI 1.0 channel message, as the host passes it to a plugin. `channel` is 0..=15.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiEvent {
    NoteOn {
        channel: u8,
        key: u8,
        velocity: u8,
    },
    NoteOff {
        channel: u8,
        key: u8,
        velocity: u8,
    },
    PolyPressure {
        channel: u8,
        key: u8,
        pressure: u8,
    },
    ControlChange {
        channel: u8,
        controller: u8,
        value: u8,
    },
    ChannelPressure {
        channel: u8,
        pressure: u8,
    },
    /// 14-bit value, 8192 = centre.
    PitchBend {
        channel: u8,
        value: u16,
    },
}

impl MidiEvent {
    /// Parses one raw channel message. Note-on with velocity 0 is a note-off. System messages
    /// (clock, sysex, active sensing, ...) give `None`.
    #[must_use]
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let (&status, data) = bytes.split_first()?;
        let channel = status & 0x0F;
        let d = |i: usize| data.get(i).map(|b| b & 0x7F);
        Some(match status & 0xF0 {
            0x80 => Self::NoteOff {
                channel,
                key: d(0)?,
                velocity: d(1)?,
            },
            0x90 if d(1)? == 0 => Self::NoteOff {
                channel,
                key: d(0)?,
                velocity: 64,
            },
            0x90 => Self::NoteOn {
                channel,
                key: d(0)?,
                velocity: d(1)?,
            },
            0xA0 => Self::PolyPressure {
                channel,
                key: d(0)?,
                pressure: d(1)?,
            },
            0xB0 => Self::ControlChange {
                channel,
                controller: d(0)?,
                value: d(1)?,
            },
            0xD0 => Self::ChannelPressure {
                channel,
                pressure: d(0)?,
            },
            0xE0 => Self::PitchBend {
                channel,
                value: u16::from(d(0)?) | (u16::from(d(1)?) << 7),
            },
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_channel_messages() {
        assert_eq!(
            MidiEvent::parse(&[0x91, 60, 100]),
            Some(MidiEvent::NoteOn {
                channel: 1,
                key: 60,
                velocity: 100
            })
        );
        assert_eq!(
            MidiEvent::parse(&[0x90, 60, 0]),
            Some(MidiEvent::NoteOff {
                channel: 0,
                key: 60,
                velocity: 64
            })
        );
        assert_eq!(
            MidiEvent::parse(&[0xB0, 64, 127]),
            Some(MidiEvent::ControlChange {
                channel: 0,
                controller: 64,
                value: 127
            })
        );
        assert_eq!(
            MidiEvent::parse(&[0xE0, 0, 64]),
            Some(MidiEvent::PitchBend {
                channel: 0,
                value: 8192
            })
        );
        assert_eq!(MidiEvent::parse(&[0xF8]), None);
        assert_eq!(MidiEvent::parse(&[0x90, 60]), None);
    }
}
