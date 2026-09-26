//! Magnetism (Hall Effect) related types for trigger settings

use crate::settings::Precision;

/// Travel distance in raw firmware units
///
/// Provides type-safe conversion to/from millimeters based on firmware precision.
/// The raw value is stored as u16 and represents travel distance in precision-dependent units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TravelDepth(u16);

impl TravelDepth {
    /// Create from raw firmware value
    pub const fn from_raw(raw: u16) -> Self {
        Self(raw)
    }

    /// Parse a user-supplied millimetre value, rounding to the nearest raw unit.
    ///
    /// This is the *only* place a float becomes a travel value: everything
    /// downstream — stepping, storing, sending, displaying — is integer, so this
    /// is the single point where a rounding decision is made.
    pub fn from_mm(mm: f32, precision: Precision) -> Self {
        Self(precision.mm_to_raw(mm as f64))
    }

    /// Get the raw firmware value
    pub const fn raw(&self) -> u16 {
        self.0
    }

    /// Convert to millimetres for *computation* (chart axes, comparisons).
    /// For display use [`Self::format`], which is exact.
    pub fn to_mm(&self, precision: Precision) -> f32 {
        precision.raw_to_mm(self.0) as f32
    }

    /// Millimetres as fixed point, e.g. `"1.50"` — pure integer arithmetic, so
    /// the displayed digits are the stored value and nothing is rounded away.
    /// The number of decimals follows the device precision.
    pub fn format_mm(&self, precision: Precision) -> String {
        let decimals = precision.decimals();
        // µm are exact; scale down to the device's own resolution, also exact.
        let scaled = precision.raw_to_um(self.0) / 10u32.pow(3 - decimals);
        let unit = 10u32.pow(decimals);
        format!(
            "{}.{:0width$}",
            scaled / unit,
            scaled % unit,
            width = decimals as usize
        )
    }

    /// Fixed-point millimetres with the unit suffix, e.g. `"1.50mm"`.
    pub fn format(&self, precision: Precision) -> String {
        format!("{}mm", self.format_mm(precision))
    }
}

impl From<u16> for TravelDepth {
    fn from(raw: u16) -> Self {
        Self(raw)
    }
}

impl From<TravelDepth> for u16 {
    fn from(depth: TravelDepth) -> Self {
        depth.0
    }
}

/// Key depth event from magnetism report
#[derive(Debug, Clone)]
pub struct KeyDepthEvent {
    /// Key matrix index
    pub key_index: u8,
    /// Raw depth value from sensor
    pub depth_raw: u16,
    /// Depth in mm (requires precision factor)
    pub depth_mm: f32,
}

/// Per-key trigger settings
#[derive(Debug, Clone, Default)]
pub struct TriggerSettings {
    /// Number of keys
    pub key_count: usize,
    /// Press travel (actuation point) per key, in raw u16 firmware units
    pub press_travel: Vec<u16>,
    /// Lift travel (release point) per key
    pub lift_travel: Vec<u16>,
    /// Rapid Trigger press sensitivity per key
    pub rt_press: Vec<u16>,
    /// Rapid Trigger lift sensitivity per key
    pub rt_lift: Vec<u16>,
    /// Key mode per key (0=Normal, 2=DKS, 3=MT, etc.)
    pub key_modes: Vec<u8>,
    /// Bottom deadzone per key
    pub bottom_deadzone: Vec<u16>,
    /// Top deadzone per key
    pub top_deadzone: Vec<u16>,
}

impl TriggerSettings {
    /// Create empty settings for a given key count
    pub fn new(key_count: usize) -> Self {
        Self {
            key_count,
            press_travel: vec![0; key_count],
            lift_travel: vec![0; key_count],
            rt_press: vec![0; key_count],
            rt_lift: vec![0; key_count],
            key_modes: vec![0; key_count],
            bottom_deadzone: vec![0; key_count],
            top_deadzone: vec![0; key_count],
        }
    }

    /// Decode a raw byte buffer of LE u16 pairs into a Vec<u16>
    pub fn decode_u16_values(bytes: &[u8], key_count: usize) -> Vec<u16> {
        bytes
            .chunks_exact(2)
            .take(key_count)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect()
    }

    /// Get settings for a specific key
    pub fn get_key(&self, index: usize) -> Option<KeyTriggerSettingsDetail> {
        if index >= self.key_count {
            return None;
        }
        let mode_byte = ModeByte::from_u8(self.key_modes.get(index).copied().unwrap_or(0));
        Some(KeyTriggerSettingsDetail {
            press_travel: self.press_travel.get(index).copied().unwrap_or(0),
            lift_travel: self.lift_travel.get(index).copied().unwrap_or(0),
            rt_press: self.rt_press.get(index).copied().unwrap_or(0),
            rt_lift: self.rt_lift.get(index).copied().unwrap_or(0),
            key_mode: mode_byte.base,
            rapid_trigger: mode_byte.rapid_trigger,
            bottom_deadzone: self.bottom_deadzone.get(index).copied().unwrap_or(0),
            top_deadzone: self.top_deadzone.get(index).copied().unwrap_or(0),
        })
    }
}

/// Simple trigger settings for single key get/set operations
#[derive(Debug, Clone, Default)]
pub struct KeyTriggerSettings {
    /// Key matrix index
    pub key_index: u8,
    /// Actuation point (raw u16 firmware units, precision-dependent)
    pub actuation: u16,
    /// Deactuation / release point (raw u16 firmware units)
    pub deactuation: u16,
    /// Rapid-Trigger press sensitivity (raw u16 firmware units)
    ///
    /// The firmware stores sensitivity per key (sub-command 0x02), so it can be
    /// written per key; the board-wide setters are the ones that fan out to
    /// every position.
    pub rt_press: u16,
    /// Rapid-Trigger lift sensitivity (raw u16 firmware units, sub-command 0x03)
    pub rt_lift: u16,
    /// Base key mode
    pub mode: KeyMode,
    /// Rapid-Trigger flag (orthogonal to `mode`)
    pub rapid_trigger: bool,
}

/// Detailed settings for a single key (from bulk query)
#[derive(Debug, Clone, Default)]
pub struct KeyTriggerSettingsDetail {
    /// Press travel (actuation point) in raw u16 firmware units
    pub press_travel: u16,
    /// Lift travel (release point)
    pub lift_travel: u16,
    /// Rapid Trigger press sensitivity
    pub rt_press: u16,
    /// Rapid Trigger lift sensitivity
    pub rt_lift: u16,
    /// Base key mode
    pub key_mode: KeyMode,
    /// Rapid-Trigger flag (orthogonal to `key_mode`)
    pub rapid_trigger: bool,
    /// Bottom deadzone
    pub bottom_deadzone: u16,
    /// Top deadzone
    pub top_deadzone: u16,
}

/// Per-key base trigger mode — the low 7 bits of the firmware mode byte.
///
/// The Rapid-Trigger ("fire") flag is the orthogonal `0x80` bit and combines
/// with *any* base mode; it is modelled separately by [`ModeByte`]. Values match
/// the official webapp decoder (`CommonKBRY5088.js::_decodeMagnetismKeyModes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyMode {
    /// Normal mode - simple actuation/release points
    #[default]
    Normal,
    /// Dynamic Keystroke (DKS)
    DynamicKeystroke,
    /// Mod-Tap
    ModTap,
    /// Toggle Hold
    ToggleHold,
    /// Toggle Dots
    ToggleDots,
    /// Snap Tap (SOCD)
    SnapTap,
    /// Unrecognized base mode (low 7 bits)
    Unknown(u8),
}

impl KeyMode {
    /// Every configurable base mode, in display/cycle order.
    pub const ALL: [KeyMode; 6] = [
        Self::Normal,
        Self::DynamicKeystroke,
        Self::ModTap,
        Self::ToggleHold,
        Self::ToggleDots,
        Self::SnapTap,
    ];

    /// Parse the base mode from the low 7 bits (ignores the `0x80` RT flag).
    pub fn from_u8(value: u8) -> Self {
        match value & 0x7F {
            0 => Self::Normal,
            2 => Self::DynamicKeystroke,
            3 => Self::ModTap,
            4 => Self::ToggleHold,
            5 => Self::ToggleDots,
            7 => Self::SnapTap,
            other => Self::Unknown(other),
        }
    }

    /// Base mode value (low 7 bits; no RT flag).
    pub fn to_u8(self) -> u8 {
        match self {
            Self::Normal => 0,
            Self::DynamicKeystroke => 2,
            Self::ModTap => 3,
            Self::ToggleHold => 4,
            Self::ToggleDots => 5,
            Self::SnapTap => 7,
            Self::Unknown(v) => v & 0x7F,
        }
    }

    /// Human-readable label — the single source of truth for mode naming.
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::DynamicKeystroke => "DKS",
            Self::ModTap => "Mod-Tap",
            Self::ToggleHold => "Toggle-Hold",
            Self::ToggleDots => "Toggle-Dots",
            Self::SnapTap => "SnapTap",
            Self::Unknown(_) => "Unknown",
        }
    }
}

impl std::fmt::Display for KeyMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// A full per-key mode byte: a [`KeyMode`] base plus the orthogonal
/// Rapid-Trigger flag (`0x80`). RT can be enabled on top of any base mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModeByte {
    /// Base mode (low 7 bits)
    pub base: KeyMode,
    /// Rapid-Trigger ("fire") flag (`0x80`)
    pub rapid_trigger: bool,
}

impl ModeByte {
    /// Rapid-Trigger flag bit.
    pub const RT_FLAG: u8 = 0x80;

    pub fn new(base: KeyMode, rapid_trigger: bool) -> Self {
        Self {
            base,
            rapid_trigger,
        }
    }

    /// Split a raw mode byte into base mode + RT flag.
    pub fn from_u8(value: u8) -> Self {
        Self {
            base: KeyMode::from_u8(value),
            rapid_trigger: value & Self::RT_FLAG != 0,
        }
    }

    /// Combine base mode + RT flag back into the raw mode byte.
    pub fn to_u8(self) -> u8 {
        self.base.to_u8() | if self.rapid_trigger { Self::RT_FLAG } else { 0 }
    }
}

impl std::fmt::Display for ModeByte {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.base.label())?;
        if self.rapid_trigger {
            f.write_str("+RT")?;
        }
        Ok(())
    }
}

/// One of four fixed travel phases on the DKS timeline (vendor grid columns).
///
/// Confirmed from the webapp DKS editor column headers (`data-cell-index` 0–3):
/// shallow columns use **触发点行程** (`dynamicTravel`, SET subcmd 0x04); full columns
/// use the key's **press actuation travel** (SET subcmd 0x00, displayed as 最大行程).
///
/// | Index | Marketing | Direction | Depth source |
/// |-------|-----------|-----------|--------------|
/// | 0 | R1 light press | Press down | Trigger-point travel |
/// | 1 | R2 deep press | Press down | Key actuation / full travel |
/// | 2 | R3 initial lift | Release up | Key actuation / full travel |
/// | 3 | R4 full release | Release up | Trigger-point travel |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DksPhase {
    PressShallow = 0,
    PressFull = 1,
    ReleaseFull = 2,
    ReleaseShallow = 3,
}

impl DksPhase {
    pub const ALL: [Self; 4] = [
        Self::PressShallow,
        Self::PressFull,
        Self::ReleaseFull,
        Self::ReleaseShallow,
    ];

    pub fn from_index(i: usize) -> Option<Self> {
        Self::ALL.get(i).copied()
    }

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn direction_label(self) -> &'static str {
        match self {
            Self::PressShallow | Self::PressFull => "press",
            Self::ReleaseFull | Self::ReleaseShallow => "release",
        }
    }

    pub fn depth_source(self) -> &'static str {
        match self {
            Self::PressShallow | Self::ReleaseShallow => "trigger_point",
            Self::PressFull | Self::ReleaseFull => "actuation",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::PressShallow => "press@trigger",
            Self::PressFull => "press@full",
            Self::ReleaseFull => "release@full",
            Self::ReleaseShallow => "release@trigger",
        }
    }

    pub fn marketing_label(self) -> &'static str {
        match self {
            Self::PressShallow => "R1 light press",
            Self::PressFull => "R2 deep press",
            Self::ReleaseFull => "R3 initial lift",
            Self::ReleaseShallow => "R4 full release",
        }
    }
}

impl std::fmt::Display for DksPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.short_label())
    }
}

/// 2-bit segment role at one [`DksPhase`] stop (vendor `RO` bar editor).
///
/// Values match the vendor webapp DKS editor (`RO` component) and help text
/// (`弹窗_动态键程提示文本`): click "+" for single trigger, drag to next "+"
/// for continuous-until-next, drag across multiple "+" for continuous-across.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum DksAction {
    #[default]
    None = 0,
    /// Click "+" — fire once at this depth.
    SingleTrigger = 1,
    /// Drag to next "+" — hold until released at the next checkpoint.
    ContinuousUntilNext = 2,
    /// Drag across multiple "+" — hold across checkpoints.
    ContinuousAcross = 3,
}

impl DksAction {
    pub fn from_u8(v: u8) -> Self {
        match v & 3 {
            1 => Self::SingleTrigger,
            2 => Self::ContinuousUntilNext,
            3 => Self::ContinuousAcross,
            _ => Self::None,
        }
    }
}

impl std::fmt::Display for DksAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::None => "none",
            Self::SingleTrigger => "single",
            Self::ContinuousUntilNext => "until_next",
            Self::ContinuousAcross => "across",
        })
    }
}

/// One of four DKS output bindings (vendor grid row) on a physical key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DksBinding {
    /// The raw 4-byte keymatrix entry this DKS slot emits, `[config_type, b1, b2, b3]`.
    ///
    /// DKS phases dispatch through the same `keycode_dispatch` as an ordinary
    /// keypress (firmware reads sub-layers 0–3 for the key), so a slot may hold any
    /// action — a chord, a consumer usage, a macro — not just plain keys. Semantics
    /// live in the driver's `KeyAction`; this crate only moves the bytes.
    pub config: [u8; 4],
    /// Segment roles at each [`DksPhase`] stop (indexed by [`DksPhase::index`]).
    pub phase_actions: [DksAction; 4],
}

impl DksBinding {
    pub fn action_at(self, phase: DksPhase) -> DksAction {
        self.phase_actions[phase.index()]
    }

    pub fn set_action_at(&mut self, phase: DksPhase, action: DksAction) {
        self.phase_actions[phase.index()] = action;
    }

    /// Pack four 2-bit phase actions into one firmware byte (per binding row).
    ///
    /// Wire: `byte = (p3<<6)|(p2<<4)|(p1<<2)|p0` where pN is [`DksPhase`] N.
    pub fn pack_phase_actions(actions: [DksAction; 4]) -> u8 {
        (actions[DksPhase::ReleaseShallow.index()] as u8) << 6
            | (actions[DksPhase::ReleaseFull.index()] as u8) << 4
            | (actions[DksPhase::PressFull.index()] as u8) << 2
            | actions[DksPhase::PressShallow.index()] as u8
    }

    /// Unpack one firmware byte into four phase-indexed actions.
    pub fn unpack_phase_actions(byte: u8) -> [DksAction; 4] {
        [
            DksAction::from_u8(byte),
            DksAction::from_u8(byte >> 2),
            DksAction::from_u8(byte >> 4),
            DksAction::from_u8(byte >> 6),
        ]
    }

    pub fn packed_mode(self) -> u8 {
        Self::pack_phase_actions(self.phase_actions)
    }

    pub fn from_packed_mode(byte: u8, config: [u8; 4]) -> Self {
        Self {
            config,
            phase_actions: Self::unpack_phase_actions(byte),
        }
    }

    /// True when this slot emits nothing at all.
    pub fn is_empty(self) -> bool {
        self.config == [0; 4]
    }
}

/// Full DKS configuration for one matrix key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DksConfig {
    /// Shallow trigger-point travel (触发点行程 / `dynamicTravel`, SET subcmd 0x04).
    pub trigger_point_travel_raw: u16,
    /// Four output bindings (grid rows / firmware `dks_binding_row0..3` packed bytes).
    pub bindings: [DksBinding; 4],
}

impl DksConfig {
    /// Four packed binding-row bytes (SET subcmd 0x08 / GET subcmd 0x0A layout).
    ///
    /// Firmware RAM names these `dks_point1..4` per key — they are **binding rows**,
    /// not travel phases. Phase stops live inside each packed byte.
    pub fn trigger_modes(&self) -> [u8; 4] {
        [
            self.bindings[0].packed_mode(),
            self.bindings[1].packed_mode(),
            self.bindings[2].packed_mode(),
            self.bindings[3].packed_mode(),
        ]
    }

    /// Parse binding-row bytes from the 512-byte GET_DKS_MODES blob for `key_index`.
    pub fn trigger_modes_from_blob(blob: &[u8], key_index: usize) -> [u8; 4] {
        let k = key_index.min(127);
        [
            blob.get(k).copied().unwrap_or(0),
            blob.get(128 + k).copied().unwrap_or(0),
            blob.get(256 + k).copied().unwrap_or(0),
            blob.get(384 + k).copied().unwrap_or(0),
        ]
    }

    pub fn from_parts(
        trigger_point_travel_raw: u16,
        modes: [u8; 4],
        configs: [[u8; 4]; 4],
    ) -> Self {
        let bindings = std::array::from_fn(|i| DksBinding::from_packed_mode(modes[i], configs[i]));
        Self {
            trigger_point_travel_raw,
            bindings,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_byte_round_trips_all_bases_and_rt() {
        for base in KeyMode::ALL {
            for rapid_trigger in [false, true] {
                let mb = ModeByte::new(base, rapid_trigger);
                let byte = mb.to_u8();
                assert_eq!(byte & ModeByte::RT_FLAG != 0, rapid_trigger);
                assert_eq!(ModeByte::from_u8(byte), mb);
                // Base value never sets the RT bit itself.
                assert_eq!(base.to_u8() & ModeByte::RT_FLAG, 0);
            }
        }
    }

    #[test]
    fn known_wire_values() {
        // Values per CommonKBRY5088.js::_decodeMagnetismKeyModes.
        assert_eq!(KeyMode::Normal.to_u8(), 0);
        assert_eq!(KeyMode::DynamicKeystroke.to_u8(), 2);
        assert_eq!(KeyMode::ModTap.to_u8(), 3);
        assert_eq!(KeyMode::ToggleHold.to_u8(), 4);
        assert_eq!(KeyMode::ToggleDots.to_u8(), 5);
        assert_eq!(KeyMode::SnapTap.to_u8(), 7);
        // DKS + Rapid Trigger, the byte the old model collapsed to bare RT.
        let dks_rt = ModeByte::from_u8(0x82);
        assert_eq!(dks_rt.base, KeyMode::DynamicKeystroke);
        assert!(dks_rt.rapid_trigger);
        assert_eq!(dks_rt.to_u8(), 0x82);
    }

    #[test]
    fn unknown_base_preserved_without_rt_bit() {
        let mb = ModeByte::from_u8(0x86); // base 6 (unknown) + RT
        assert_eq!(mb.base, KeyMode::Unknown(6));
        assert!(mb.rapid_trigger);
        assert_eq!(mb.to_u8(), 0x86);
    }

    #[test]
    fn dks_action_pack_roundtrip() {
        let actions = [
            DksAction::SingleTrigger,
            DksAction::None,
            DksAction::ContinuousUntilNext,
            DksAction::ContinuousAcross,
        ];
        let packed = DksBinding::pack_phase_actions(actions);
        assert_eq!(packed, 0b11_10_00_01);
        assert_eq!(DksBinding::unpack_phase_actions(packed), actions);
    }

    #[test]
    fn dks_trigger_modes_blob_layout() {
        let mut blob = vec![0u8; 512];
        blob[5] = 0x55;
        blob[128 + 5] = 0xAA;
        blob[256 + 5] = 0x0F;
        blob[384 + 5] = 0xF0;
        assert_eq!(
            DksConfig::trigger_modes_from_blob(&blob, 5),
            [0x55, 0xAA, 0x0F, 0xF0]
        );
    }

    #[test]
    fn dks_binding_carries_raw_config_bytes() {
        // Ctrl+A+C as three usages, exactly as the vendor's configToMatrix emits it.
        let b = DksBinding::from_packed_mode(0, [0, 0xE0, 0x04, 0x06]);
        assert_eq!(b.config, [0, 0xE0, 0x04, 0x06]);
        assert!(!b.is_empty());
        // A DKS slot may also hold a non-key action, e.g. Macro(3).
        assert_eq!(
            DksBinding::from_packed_mode(0, [9, 0, 3, 0]).config,
            [9, 0, 3, 0]
        );
        assert!(DksBinding::from_packed_mode(0, [0; 4]).is_empty());
    }
}

/// `TravelDepth` is the only float entry point for travel, and it rounds.
/// The call sites it replaced used `(mm * factor) as u16`, which truncates.
#[cfg(test)]
mod travel_depth_tests {
    use super::TravelDepth;
    use crate::settings::Precision;

    /// A millimetre value whose f32 product falls just under a raw unit must
    /// round up, not truncate. `(mm * factor) as u16` stored 1.05mm as 104 and
    /// 0.53mm as 52 — a unit shallower than the user asked for. Fourteen of the
    /// 401 two-decimal values in 0.00..=4.00mm are affected at 0.01mm precision,
    /// and 184 of them at 0.1mm.
    #[test]
    fn from_mm_rounds_rather_than_truncating() {
        let p = Precision::Medium; // 0.01mm per raw unit
        for (mm, expect) in [(1.05f32, 105u16), (0.53, 53), (1.06, 106), (1.17, 117)] {
            assert_eq!(TravelDepth::from_mm(mm, p).raw(), expect);
            // The form this replaced landed one unit low:
            assert_eq!(
                (mm * 100.0) as u16,
                expect - 1,
                "{mm} was not a truncation witness"
            );
        }
        // Values that were already exact stay exact.
        assert_eq!(TravelDepth::from_mm(1.5, p).raw(), 150);
        assert_eq!(TravelDepth::from_mm(0.3, p).raw(), 30);
        assert_eq!(TravelDepth::from_mm(2.0, p).raw(), 200);
    }

    /// Every raw value round-trips through mm at every precision, so displaying a
    /// stored value and reading it back cannot shift it.
    #[test]
    fn raw_survives_a_round_trip_through_mm() {
        for p in [Precision::Coarse, Precision::Medium, Precision::Fine] {
            for raw in [0u16, 1, 30, 150, 204, 205, 400, 800] {
                let d = TravelDepth::from_raw(raw);
                assert_eq!(
                    TravelDepth::from_mm(d.to_mm(p), p).raw(),
                    raw,
                    "{p:?}: raw {raw} did not survive"
                );
            }
        }
    }

    /// Display is integer fixed point, so every raw value renders exactly and
    /// parsing the rendered string returns the same raw value. A float `{:.2}`
    /// render could not do this at 0.005mm precision, where a raw unit is half
    /// of the last digit shown.
    #[test]
    fn display_is_exact_and_reparses_to_the_same_raw() {
        for p in [Precision::Coarse, Precision::Medium, Precision::Fine] {
            for raw in 0..=800u16 {
                let d = TravelDepth::from_raw(raw);
                let text = d.format_mm(p);
                let reparsed: f32 = text.parse().expect("fixed point parses as a number");
                assert_eq!(
                    TravelDepth::from_mm(reparsed, p).raw(),
                    raw,
                    "{p:?}: raw {raw} rendered as {text}, which reads back differently"
                );
            }
        }
    }

    #[test]
    fn display_follows_the_device_resolution() {
        let d = TravelDepth::from_raw(204);
        assert_eq!(d.format(Precision::Coarse), "20.4mm"); // 0.1mm units
        assert_eq!(d.format(Precision::Medium), "2.04mm"); // 0.01mm units
        assert_eq!(d.format(Precision::Fine), "1.020mm"); // 0.005mm units
        // A trailing zero is kept, so the column width does not jump around.
        assert_eq!(
            TravelDepth::from_raw(200).format(Precision::Medium),
            "2.00mm"
        );
        assert_eq!(TravelDepth::from_raw(0).format(Precision::Medium), "0.00mm");
    }
}
