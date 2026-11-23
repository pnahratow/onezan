//! One plugin instance: creation, wiring of component and controller, activation, processing.

use crate::editor::Editor;
use crate::events::{EventList, ParamChanges};
use crate::host::HostApp;
use crate::midi::MidiEvent;
use crate::module::{Module, RawClass};
use crate::stream::MemoryStream;
use crate::util::{class_id_string, int32, ok, tuid_bytes, venum};
use crate::{Error, Result};
use std::path::Path;
use std::sync::Arc;
use vst3::Steinberg::Vst::{
    AudioBusBuffers, AudioBusBuffers__type0, BusDirections_, BusInfo, Event, Event_, IAudioProcessor,
    IAudioProcessorTrait, IComponent, IComponentHandler, IComponentTrait, IConnectionPoint, IConnectionPointTrait,
    IEditController, IEditControllerTrait, IEventList, IHostApplication, IMidiMapping, IMidiMappingTrait,
    IParameterChanges, MediaTypes_, NoteOffEvent, NoteOnEvent, PolyPressureEvent, ProcessContext, ProcessContext_,
    ProcessData, ProcessModes_, ProcessSetup, SpeakerArr, SpeakerArrangement, SymbolicSampleSizes_,
};
use vst3::Steinberg::{FUnknown, IBStream, IPluginBaseTrait, TUID};
use vst3::{ComPtr, ComWrapper};

/// VST3's pseudo controller numbers for MIDI messages that are not CCs (`ControllerNumbers`).
const CTRL_AFTERTOUCH: u8 = 128;
const CTRL_PITCH_BEND: u8 = 129;
const MAP_CTRLS: usize = 130;

/// A loaded plugin instance (main thread).
pub struct Plugin {
    name: String,
    /// The class id in hex: names this plugin across versions and installs.
    class_id: String,
    component: ComPtr<IComponent>,
    processor: ComPtr<IAudioProcessor>,
    controller: Option<ComPtr<IEditController>>,
    /// The controller is its own object (initialised and terminated by us).
    controller_separate: bool,
    connections: Option<(ComPtr<IConnectionPoint>, ComPtr<IConnectionPoint>)>,
    host: ComWrapper<HostApp>,
    _module: Arc<Module>,
    active: bool,
    /// Held by the live [`Processor`]; the plugin is not deactivated while it is.
    processor_token: Arc<()>,
    /// Held by an open [`Editor`]; the plugin is not torn down while it is.
    editor_token: Arc<()>,
}

/// Picks the class to load: the one with `class_id` (hex), else the first instrument, else the
/// first audio processor.
fn pick_class<'a>(classes: &'a [RawClass], class_id: Option<&str>) -> Option<&'a RawClass> {
    let audio = || classes.iter().filter(|c| c.category == "Audio Module Class");
    class_id.map_or_else(
        || {
            audio()
                .find(|c| c.sub_categories.split('|').any(|s| s == "Instrument"))
                .or_else(|| audio().next())
        },
        |id| {
            classes
                .iter()
                .find(|c| class_id_string(&c.cid).eq_ignore_ascii_case(id))
        },
    )
}

impl Plugin {
    /// Loads `bundle` and creates one instance of the class `class_id` (hex), or of its first
    /// instrument when `None`.
    ///
    /// # Errors
    ///
    /// When the bundle cannot be loaded, has no matching class, or the plugin fails to
    /// initialise.
    pub fn load(bundle: &Path, class_id: Option<&str>) -> Result<Self> {
        let module = Module::load(bundle)?;
        let classes = module.classes();
        let class = pick_class(&classes, class_id).ok_or_else(|| {
            Error::Instantiate(bundle.display().to_string(), "no matching audio processor class".into())
        })?;
        let name = class.name.clone();
        let class_id = class_id_string(&class.cid);
        let err = |m: &str| Error::Instantiate(name.clone(), m.to_string());

        let host = ComWrapper::new(HostApp);
        let host_unknown = host
            .as_com_ref::<IHostApplication>()
            .ok_or_else(|| err("host context"))?
            .upcast::<FUnknown>()
            .as_ptr();
        let component = module
            .create::<IComponent>(&class.cid)
            .ok_or_else(|| err("factory cannot create the component"))?;
        // SAFETY: fresh component; the host context outlives it (owned by the Plugin).
        if !ok(unsafe { component.initialize(host_unknown) }) {
            return Err(err("component initialize failed"));
        }
        let Some(processor) = component.cast::<IAudioProcessor>() else {
            // SAFETY: initialised above, terminated once.
            unsafe { component.terminate() };
            return Err(err("component has no IAudioProcessor"));
        };

        let mut plugin = Self {
            name: name.clone(),
            class_id,
            component,
            processor,
            controller: None,
            controller_separate: false,
            connections: None,
            host,
            _module: module.clone(),
            active: false,
            processor_token: Arc::new(()),
            editor_token: Arc::new(()),
        };

        // The edit controller is either the component itself or a separate class.
        if let Some(c) = plugin.component.cast::<IEditController>() {
            plugin.controller = Some(c);
        } else {
            let mut ccid: TUID = [0; 16];
            // SAFETY: valid component; `ccid` is a writable TUID.
            if ok(unsafe { plugin.component.getControllerClassId(&raw mut ccid) })
                && let Some(c) = module.create::<IEditController>(&tuid_bytes(&ccid))
            {
                // SAFETY: fresh controller; the host context outlives it.
                if ok(unsafe { c.initialize(host_unknown) }) {
                    plugin.controller = Some(c);
                    plugin.controller_separate = true;
                } else {
                    log::warn!("vst3 {name}: controller initialize failed; MIDI CC mapping unavailable");
                }
            }
        }
        if let Some(c) = &plugin.controller {
            if plugin.controller_separate
                && let (Some(a), Some(b)) = (
                    plugin.component.cast::<IConnectionPoint>(),
                    c.cast::<IConnectionPoint>(),
                )
            {
                // SAFETY: both points live as long as the Plugin; disconnected in Drop.
                unsafe {
                    a.connect(b.as_ptr());
                    b.connect(a.as_ptr());
                }
                plugin.connections = Some((a, b));
            }
            if let Some(h) = plugin.host.as_com_ref::<IComponentHandler>() {
                // SAFETY: valid controller; the handler outlives it.
                unsafe { c.setComponentHandler(h.as_ptr()) };
            }
        }
        // The controller starts in step with the component (its parameters show the sound).
        if let Ok(state) = plugin.component_state() {
            plugin.set_component_state_on_controller(state);
        }
        Ok(plugin)
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The class id in hex, to file things by this plugin under (its saved state).
    #[must_use]
    pub fn class_id(&self) -> &str {
        &self.class_id
    }

    /// The plugin's current state: its sound and settings, as the plugin itself writes them.
    /// Main thread; fine while audio runs.
    ///
    /// # Errors
    ///
    /// When the component cannot write its state.
    pub fn state(&self) -> Result<State> {
        let component = self.component_state()?;
        let controller = self
            .controller
            .as_ref()
            .and_then(|c| {
                let stream = ComWrapper::new(MemoryStream::default());
                // SAFETY: valid controller; the stream lives through the call.
                ok(unsafe { c.getState(stream_ptr(&stream)) }).then(|| stream.bytes())
            })
            .unwrap_or_default();
        Ok(State { component, controller })
    }

    /// Restores a state from [`Self::state`], in the order the VST3 SDK's hosts use: the
    /// component, then the component's state to the controller, then the controller's own.
    /// Main thread; call it before [`Self::start`].
    ///
    /// # Errors
    ///
    /// When the component refuses the state (another version, or not its own).
    pub fn set_state(&mut self, state: &State) -> Result<()> {
        let stream = ComWrapper::new(MemoryStream::from_bytes(state.component.clone()));
        // SAFETY: valid component; the stream lives through the call.
        if !ok(unsafe { self.component.setState(stream_ptr(&stream)) }) {
            return Err(Error::Instantiate(
                self.name.clone(),
                "the plugin refused the saved state".into(),
            ));
        }
        self.set_component_state_on_controller(state.component.clone());
        if let Some(c) = &self.controller
            && !state.controller.is_empty()
        {
            let stream = ComWrapper::new(MemoryStream::from_bytes(state.controller.clone()));
            // SAFETY: valid controller; the stream lives through the call.
            if !ok(unsafe { c.setState(stream_ptr(&stream)) }) {
                log::warn!("vst3 {}: the controller refused its saved state", self.name);
            }
        }
        Ok(())
    }

    fn component_state(&self) -> Result<Vec<u8>> {
        let stream = ComWrapper::new(MemoryStream::default());
        // SAFETY: valid component; the stream lives through the call.
        if ok(unsafe { self.component.getState(stream_ptr(&stream)) }) {
            Ok(stream.bytes())
        } else {
            Err(Error::Instantiate(
                self.name.clone(),
                "the plugin cannot save its state".into(),
            ))
        }
    }

    fn set_component_state_on_controller(&self, component: Vec<u8>) {
        if let Some(c) = &self.controller {
            let stream = ComWrapper::new(MemoryStream::from_bytes(component));
            // SAFETY: valid controller; the stream lives through the call.
            if !ok(unsafe { c.setComponentState(stream_ptr(&stream)) }) {
                log::info!("vst3 {}: the controller did not take the component's state", self.name);
            }
        }
    }

    /// The plugin's own editor, ready to be embedded in a window of the app. Main thread. Drop
    /// it before the plugin.
    ///
    /// # Errors
    ///
    /// When the plugin has no editor, or none for this platform's windows.
    pub fn editor(&self) -> Result<Editor> {
        let controller = self
            .controller
            .as_ref()
            .ok_or_else(|| Error::Editor("the plugin has no edit controller".into()))?;
        Editor::new(controller, Arc::clone(&self.editor_token))
    }

    fn bus_count(&self, media: i32, dir: i32) -> i32 {
        // SAFETY: valid component.
        unsafe { self.component.getBusCount(media, dir) }.clamp(0, 16)
    }

    fn bus_channels(&self, dir: i32, index: i32) -> usize {
        // SAFETY: plain data; all-zero is valid.
        let mut info: BusInfo = unsafe { std::mem::zeroed() };
        // SAFETY: valid component; index below the bus count; `info` is writable.
        if ok(unsafe {
            self.component
                .getBusInfo(venum(MediaTypes_::kAudio), dir, index, &raw mut info)
        }) {
            usize::try_from(info.channelCount.clamp(0, 32)).unwrap_or(0)
        } else {
            0
        }
    }

    fn arrangement(&self, dir: i32, index: i32) -> SpeakerArrangement {
        let mut a: SpeakerArrangement = SpeakerArr::kStereo;
        // SAFETY: valid processor; `a` is writable.
        unsafe { self.processor.getBusArrangement(dir, index, &raw mut a) };
        a
    }

    /// Configures and activates the plugin for `sample_rate` and blocks of up to `max_frames`,
    /// and returns the audio-thread half.
    ///
    /// # Errors
    ///
    /// When a processor is still running, or the plugin has no audio output, cannot process
    /// 32-bit float, or refuses the setup or activation.
    pub fn start(&mut self, sample_rate: f64, max_frames: usize) -> Result<Processor> {
        if Arc::strong_count(&self.processor_token) > 1 {
            return Err(Error::Process("plugin is already running".into()));
        }
        self.stop();
        let (audio, event): (i32, i32) = (venum(MediaTypes_::kAudio), venum(MediaTypes_::kEvent));
        let (din, dout): (i32, i32) = (venum(BusDirections_::kInput), venum(BusDirections_::kOutput));
        let (nin, nout) = (self.bus_count(audio, din), self.bus_count(audio, dout));
        if nout == 0 {
            return Err(Error::Process("plugin has no audio output".into()));
        }

        // Ask for stereo on the main output; keep every other bus as the plugin has it.
        let mut ins: Vec<SpeakerArrangement> = (0..nin).map(|i| self.arrangement(din, i)).collect();
        let mut outs: Vec<SpeakerArrangement> = (0..nout).map(|i| self.arrangement(dout, i)).collect();
        if let Some(main) = outs.first_mut() {
            *main = SpeakerArr::kStereo;
        }
        // SAFETY: valid, inactive processor; the arrays hold exactly nin/nout entries.
        if !ok(unsafe {
            self.processor
                .setBusArrangements(ins.as_mut_ptr(), nin, outs.as_mut_ptr(), nout)
        }) {
            log::debug!(
                "vst3 {}: stereo output refused; using the plugin's own layout",
                self.name
            );
        }

        // Main output on; inputs and auxiliary outputs off (they still get buffers).
        for i in 0..nout {
            // SAFETY: valid, inactive component; index below the bus count.
            unsafe { self.component.activateBus(audio, dout, i, u8::from(i == 0)) };
        }
        for i in 0..nin {
            // SAFETY: as above.
            unsafe { self.component.activateBus(audio, din, i, 0) };
        }
        let has_events = self.bus_count(event, din) > 0;
        if has_events {
            // SAFETY: valid, inactive component; event input bus 0 exists.
            unsafe { self.component.activateBus(event, din, 0, 1) };
        }

        let sample32: i32 = venum(SymbolicSampleSizes_::kSample32);
        // SAFETY: valid processor.
        if !ok(unsafe { self.processor.canProcessSampleSize(sample32) }) {
            return Err(Error::Process("plugin cannot process 32-bit float".into()));
        }
        let mut setup = ProcessSetup {
            processMode: venum(ProcessModes_::kRealtime),
            symbolicSampleSize: sample32,
            maxSamplesPerBlock: int32(max_frames),
            sampleRate: sample_rate,
        };
        // SAFETY: valid, inactive processor; `setup` is valid for the call.
        if !ok(unsafe { self.processor.setupProcessing(&raw mut setup) }) {
            return Err(Error::Process("setupProcessing failed".into()));
        }
        // SAFETY: valid, configured component.
        if !ok(unsafe { self.component.setActive(1) }) {
            return Err(Error::Process("setActive failed".into()));
        }
        self.active = true;

        let in_ch: Vec<usize> = (0..nin).map(|i| self.bus_channels(din, i)).collect();
        let out_ch: Vec<usize> = (0..nout).map(|i| self.bus_channels(dout, i)).collect();
        log::info!(
            "vst3 {}: {} Hz, {} frames, inputs {in_ch:?}, outputs {out_ch:?}, events {has_events}",
            self.name,
            sample_rate,
            max_frames
        );

        // SAFETY: plain data; all-zero is valid.
        let mut context: ProcessContext = unsafe { std::mem::zeroed() };
        context.sampleRate = sample_rate;
        context.tempo = 120.0;
        context.timeSigNumerator = 4;
        context.timeSigDenominator = 4;
        context.state =
            venum(ProcessContext_::StatesAndFlags_::kTempoValid | ProcessContext_::StatesAndFlags_::kTimeSigValid);

        Ok(Processor {
            plugin: self.processor.clone(),
            in_events: ComWrapper::new(EventList::new()),
            out_events: ComWrapper::new(EventList::new()),
            in_params: ComWrapper::new(ParamChanges::new()),
            out_params: ComWrapper::new(ParamChanges::new()),
            inputs: Buses::new(&in_ch, max_frames),
            outputs: Buses::new(&out_ch, max_frames),
            context,
            max_frames,
            processing: false,
            has_events,
            midi_map: self.midi_map(),
            _token: self.processor_token.clone(),
        })
    }

    /// The controller's MIDI CC → parameter table, `[channel][controller]`.
    fn midi_map(&self) -> Box<MidiMap> {
        let mut map: Box<MidiMap> = vec![[None; MAP_CTRLS]; 16].into_boxed_slice();
        let Some(m) = self.controller.as_ref().and_then(vst3::ComPtr::cast::<IMidiMapping>) else {
            return map;
        };
        for (ch, row) in (0i16..).zip(map.iter_mut()) {
            for (ctrl, slot) in (0i16..).zip(row.iter_mut()) {
                let mut id = 0u32;
                // SAFETY: valid mapping interface; `id` is writable.
                if ok(unsafe { m.getMidiControllerAssignment(0, ch, ctrl, &raw mut id) }) {
                    *slot = Some(id);
                }
            }
        }
        map
    }

    /// Deactivates the plugin. Refused (logged) while the [`Processor`] is still alive.
    pub fn stop(&mut self) {
        if !self.active {
            return;
        }
        if Arc::strong_count(&self.processor_token) > 1 {
            log::error!(
                "vst3 {}: stop() while its processor is alive; drop the audio stream first",
                self.name
            );
            return;
        }
        // SAFETY: valid, active component.
        unsafe { self.component.setActive(0) };
        self.active = false;
    }
}

impl Drop for Plugin {
    fn drop(&mut self) {
        if Arc::strong_count(&self.editor_token) > 1 {
            log::error!(
                "vst3 {}: dropped while its editor is open; leaking the instance",
                self.name
            );
            return;
        }
        if Arc::strong_count(&self.processor_token) > 1 {
            // Terminating under a running processor would crash it: leak the instance instead.
            log::error!(
                "vst3 {}: dropped while its processor is alive; leaking the instance",
                self.name
            );
            return;
        }
        self.stop();
        if let Some((a, b)) = self.connections.take() {
            // SAFETY: connected in `load`; disconnect before terminating either side.
            unsafe {
                a.disconnect(b.as_ptr());
                b.disconnect(a.as_ptr());
            }
        }
        if let Some(c) = self.controller.take() {
            // SAFETY: valid controller; detach our handler before it goes.
            unsafe { c.setComponentHandler(std::ptr::null_mut()) };
            if self.controller_separate {
                // SAFETY: separately created and initialised once.
                unsafe { c.terminate() };
            }
        }
        // SAFETY: valid, inactive component, terminated exactly once.
        unsafe { self.component.terminate() };
    }
}

/// The stream as the `IBStream*` the plugin takes.
fn stream_ptr(s: &ComWrapper<MemoryStream>) -> *mut IBStream {
    s.as_com_ref::<IBStream>().map_or(std::ptr::null_mut(), |r| r.as_ptr())
}

/// A plugin's saved state: what its component and its controller wrote, as opaque bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    pub component: Vec<u8>,
    pub controller: Vec<u8>,
}

impl State {
    /// Marks the file format; the number changes if the layout ever does.
    const MAGIC: &[u8; 8] = b"OZVST3S1";

    /// The state as one byte string: the marker, then each part as a little-endian u64 length
    /// and its bytes.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Self::MAGIC.to_vec();
        for part in [&self.component, &self.controller] {
            out.extend_from_slice(&u64::try_from(part.len()).unwrap_or(u64::MAX).to_le_bytes());
            out.extend_from_slice(part);
        }
        out
    }

    /// Reads [`Self::to_bytes`]' output; `None` when it is not that (damaged or another format).
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let mut rest = bytes.strip_prefix(Self::MAGIC.as_slice())?;
        let mut part = || -> Option<Vec<u8>> {
            let (len, tail) = rest.split_first_chunk::<8>()?;
            let len = usize::try_from(u64::from_le_bytes(*len)).ok()?;
            let (data, tail) = tail.split_at_checked(len)?;
            rest = tail;
            Some(data.to_vec())
        };
        let component = part()?;
        let controller = part()?;
        rest.is_empty().then_some(Self { component, controller })
    }
}

/// Planar f32 buffers for every bus of one direction, plus the pointer tables VST3 reads.
struct Buses {
    data: Vec<Vec<Vec<f32>>>,
    ptrs: Vec<Vec<*mut f32>>,
    bufs: Vec<AudioBusBuffers>,
}

impl Buses {
    fn new(channels: &[usize], frames: usize) -> Self {
        let data: Vec<Vec<Vec<f32>>> = channels
            .iter()
            .map(|&c| (0..c).map(|_| vec![0.0; frames]).collect())
            .collect();
        let ptrs = data.iter().map(|b| vec![std::ptr::null_mut(); b.len()]).collect();
        let bufs = data
            .iter()
            .map(|_| AudioBusBuffers {
                numChannels: 0,
                silenceFlags: 0,
                __field0: AudioBusBuffers__type0 {
                    channelBuffers32: std::ptr::null_mut(),
                },
            })
            .collect();
        Self { data, ptrs, bufs }
    }

    /// Points the VST3 tables at the buffers (no allocation).
    fn refresh(&mut self) {
        for ((d, p), b) in self.data.iter_mut().zip(self.ptrs.iter_mut()).zip(self.bufs.iter_mut()) {
            for (dc, pc) in d.iter_mut().zip(p.iter_mut()) {
                *pc = dc.as_mut_ptr();
            }
            b.numChannels = int32(p.len());
            b.silenceFlags = 0;
            b.__field0.channelBuffers32 = p.as_mut_ptr();
        }
    }

    const fn table(&mut self) -> *mut AudioBusBuffers {
        if self.bufs.is_empty() {
            std::ptr::null_mut()
        } else {
            self.bufs.as_mut_ptr()
        }
    }
}

/// The audio-thread half of a running plugin. Move it into the audio callback.
pub struct Processor {
    plugin: ComPtr<IAudioProcessor>,
    in_events: ComWrapper<EventList>,
    out_events: ComWrapper<EventList>,
    in_params: ComWrapper<ParamChanges>,
    out_params: ComWrapper<ParamChanges>,
    inputs: Buses,
    outputs: Buses,
    context: ProcessContext,
    max_frames: usize,
    processing: bool,
    has_events: bool,
    midi_map: Box<MidiMap>,
    _token: Arc<()>,
}

// SAFETY: the processor is used by one thread at a time (moved to the audio thread); VST3
// requires `process` and `setProcessing` to be callable from the audio thread. The bus pointer
// tables only point into buffers this struct owns.
#[expect(
    clippy::non_send_fields_in_send_ty,
    reason = "COM pointers and bus pointer tables; see the SAFETY note"
)]
unsafe impl Send for Processor {}

/// MIDI controller → parameter id, `[channel][controller]`, 16 channels.
type MidiMap = [[Option<u32>; MAP_CTRLS]];

fn note_event(r#type: Event_::EventTypes, offset: i32) -> Event {
    // SAFETY: plain data (numbers and a union of plain structs); all-zero is valid.
    let mut e: Event = unsafe { std::mem::zeroed() };
    e.sampleOffset = offset;
    e.r#type = venum(r#type);
    e.flags = venum(Event_::EventFlags_::kIsLive);
    e
}

impl Processor {
    /// Hands `ev` to the plugin at frame `offset` of the coming `process` call.
    fn queue_event(&self, offset: i32, ev: MidiEvent) {
        let map = |ch: u8, ctrl: u8| {
            self.midi_map
                .get(usize::from(ch & 0x0F))
                .and_then(|row| row.get(usize::from(ctrl)))
                .copied()
                .flatten()
        };
        match ev {
            MidiEvent::NoteOn { channel, key, velocity } if self.has_events => {
                let mut e = note_event(Event_::EventTypes_::kNoteOnEvent, offset);
                e.__field0.noteOn = NoteOnEvent {
                    channel: i16::from(channel),
                    pitch: i16::from(key),
                    tuning: 0.0,
                    velocity: f32::from(velocity) / 127.0,
                    length: 0,
                    noteId: -1,
                };
                self.in_events.push(e);
            }
            MidiEvent::NoteOff { channel, key, velocity } if self.has_events => {
                let mut e = note_event(Event_::EventTypes_::kNoteOffEvent, offset);
                e.__field0.noteOff = NoteOffEvent {
                    channel: i16::from(channel),
                    pitch: i16::from(key),
                    velocity: f32::from(velocity) / 127.0,
                    noteId: -1,
                    tuning: 0.0,
                };
                self.in_events.push(e);
            }
            MidiEvent::PolyPressure { channel, key, pressure } if self.has_events => {
                let mut e = note_event(Event_::EventTypes_::kPolyPressureEvent, offset);
                e.__field0.polyPressure = PolyPressureEvent {
                    channel: i16::from(channel),
                    pitch: i16::from(key),
                    pressure: f32::from(pressure) / 127.0,
                    noteId: -1,
                };
                self.in_events.push(e);
            }
            MidiEvent::ControlChange {
                channel,
                controller,
                value,
            } => {
                if let Some(id) = map(channel, controller) {
                    self.in_params.add(id, offset, f64::from(value) / 127.0);
                }
            }
            MidiEvent::ChannelPressure { channel, pressure } => {
                if let Some(id) = map(channel, CTRL_AFTERTOUCH) {
                    self.in_params.add(id, offset, f64::from(pressure) / 127.0);
                }
            }
            MidiEvent::PitchBend { channel, value } => {
                if let Some(id) = map(channel, CTRL_PITCH_BEND) {
                    self.in_params.add(id, offset, f64::from(value) / 16383.0);
                }
            }
            _ => {}
        }
    }

    /// Renders `left.len()` frames into `left`/`right` (equal lengths). Each event is
    /// `(frame, event)`: it takes effect at that frame of `left`, the last frame if past the end.
    /// `events` must be sorted by frame. Real-time safe: no allocation, no blocking.
    pub fn process(&mut self, events: &[(usize, MidiEvent)], left: &mut [f32], right: &mut [f32]) {
        if !self.processing {
            // Many plugins return kNotImplemented here; that is fine.
            // SAFETY: valid, active processor.
            unsafe { self.plugin.setProcessing(1) };
            self.processing = true;
        }
        let block = self.max_frames.max(1);
        let total = left.len().min(right.len());
        let mut pending = events.iter().peekable();
        let mut start = 0usize;
        for (l, r_out) in left.chunks_mut(block).zip(right.chunks_mut(block)) {
            let frames = l.len().min(r_out.len());
            let end = start.saturating_add(frames);
            self.in_events.clear();
            self.in_params.clear();
            // This chunk's events; the last chunk also takes any that are past the end.
            while let Some(&(at, ev)) = pending.next_if(|(at, _)| *at < end || end >= total) {
                let offset = at.min(end.saturating_sub(1)).saturating_sub(start);
                self.queue_event(int32(offset), ev);
            }
            start = end;
            self.out_events.clear();
            self.out_params.clear();
            self.inputs.refresh();
            self.outputs.refresh();
            let mut data = ProcessData {
                processMode: venum(ProcessModes_::kRealtime),
                symbolicSampleSize: venum(SymbolicSampleSizes_::kSample32),
                numSamples: int32(frames),
                numInputs: int32(self.inputs.bufs.len()),
                numOutputs: int32(self.outputs.bufs.len()),
                inputs: self.inputs.table(),
                outputs: self.outputs.table(),
                inputParameterChanges: self
                    .in_params
                    .as_com_ref::<IParameterChanges>()
                    .map_or(std::ptr::null_mut(), |r| r.as_ptr()),
                outputParameterChanges: self
                    .out_params
                    .as_com_ref::<IParameterChanges>()
                    .map_or(std::ptr::null_mut(), |r| r.as_ptr()),
                inputEvents: self
                    .in_events
                    .as_com_ref::<IEventList>()
                    .map_or(std::ptr::null_mut(), |r| r.as_ptr()),
                outputEvents: self
                    .out_events
                    .as_com_ref::<IEventList>()
                    .map_or(std::ptr::null_mut(), |r| r.as_ptr()),
                processContext: &raw mut self.context,
            };
            // SAFETY: valid, active processor; every channel pointer refers to at least `frames`
            // f32s owned by `self`; the lists and the context outlive the call.
            let r = unsafe { self.plugin.process(&raw mut data) };
            // Mono main outputs go to both sides; a failed call gives silence.
            let main = self.outputs.data.first().filter(|_| ok(r));
            let src_l = main.and_then(|m| m.first());
            let src_r = main.and_then(|m| m.get(1)).or(src_l);
            copy_or_silence(l, src_l);
            copy_or_silence(r_out, src_r);
            let step = i64::try_from(frames).unwrap_or(0);
            self.context.projectTimeSamples = self.context.projectTimeSamples.saturating_add(step);
            self.context.continousTimeSamples = self.context.continousTimeSamples.saturating_add(step);
        }
    }
}

impl Drop for Processor {
    fn drop(&mut self) {
        if self.processing {
            // SAFETY: valid processor in the processing state.
            unsafe { self.plugin.setProcessing(0) };
        }
    }
}

/// Copies `src` into `dst`, or fills `dst` with silence when there is no source.
fn copy_or_silence(dst: &mut [f32], src: Option<&Vec<f32>>) {
    dst.fill(0.0);
    for (d, s) in dst.iter_mut().zip(src.into_iter().flatten()) {
        *d = *s;
    }
}

#[cfg(test)]
mod state_tests {
    use super::State;

    #[test]
    fn state_bytes_round_trip_and_reject_damage() {
        let s = State {
            component: b"sound".to_vec(),
            controller: vec![0, 1, 2],
        };
        let bytes = s.to_bytes();
        assert_eq!(State::from_bytes(&bytes), Some(s));
        assert_eq!(State::from_bytes(&bytes[..bytes.len() - 1]), None);
        assert_eq!(State::from_bytes(b"something else"), None);
        let empty = State::default();
        assert_eq!(State::from_bytes(&empty.to_bytes()), Some(empty));
    }
}
