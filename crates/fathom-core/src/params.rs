//! Parameter schemas.
//!
//! An app declares its parameters once with the [`params!`](crate::params) macro. That
//! single declaration produces both the schema (which the UI reads to build controls)
//! and the index constants (which the app uses to read values), so the two can never
//! drift apart.
//!
//! Values live in a flat [`ParamBlock`]: one 4-byte word per parameter, padded to a
//! multiple of 16 bytes so it can be uploaded straight into a uniform buffer. The UI
//! keeps a mirror of that block and writes the whole thing once a frame.

use serde::Serialize;

/// The storage class of a parameter. Every kind occupies exactly one word.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ParamKind {
    /// A continuous value, rendered as a slider.
    Float,
    /// A whole number, rendered as a stepped slider.
    Int,
    /// An on/off value, rendered as a toggle.
    Toggle,
    /// A one-of-N choice, rendered as a select. See [`ParamDef::options`].
    Choice,
}

/// One declared parameter: how to store it, and how to draw a control for it.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct ParamDef {
    /// Stable identifier used by controls to bind to this parameter.
    pub name: &'static str,
    /// Human-readable label shown next to the control.
    pub label: &'static str,
    pub kind: ParamKind,
    pub default: f32,
    pub min: f32,
    pub max: f32,
    /// Slider granularity. `0.0` means "continuous".
    pub step: f32,
    /// Panel section this control is filed under.
    pub group: &'static str,
    /// Labels for [`ParamKind::Choice`], indexed by value. Empty for other kinds.
    pub options: &'static [&'static str],
    /// Whether this control is filed behind its group's disclosure rather than shown
    /// with the rest.
    ///
    /// A panel that shows everything at once shows nothing in particular. Most apps have
    /// a handful of parameters worth reaching for and a long tail that exists so the
    /// first few can be trusted — this marks the tail, without hiding it.
    pub advanced: bool,
}

impl ParamDef {
    pub const fn float(name: &'static str, label: &'static str, default: f32, min: f32, max: f32) -> Self {
        Self {
            name,
            label,
            kind: ParamKind::Float,
            default,
            min,
            max,
            step: 0.0,
            group: "General",
            options: &[],
            advanced: false,
        }
    }

    pub const fn int(name: &'static str, label: &'static str, default: u32, min: u32, max: u32) -> Self {
        Self {
            name,
            label,
            kind: ParamKind::Int,
            default: default as f32,
            min: min as f32,
            max: max as f32,
            step: 1.0,
            group: "General",
            options: &[],
            advanced: false,
        }
    }

    pub const fn toggle(name: &'static str, label: &'static str, default: bool) -> Self {
        Self {
            name,
            label,
            kind: ParamKind::Toggle,
            default: if default { 1.0 } else { 0.0 },
            min: 0.0,
            max: 1.0,
            step: 1.0,
            group: "General",
            options: &[],
            advanced: false,
        }
    }

    pub const fn choice(
        name: &'static str,
        label: &'static str,
        default: u32,
        options: &'static [&'static str],
    ) -> Self {
        Self {
            name,
            label,
            kind: ParamKind::Choice,
            default: default as f32,
            min: 0.0,
            max: options.len() as f32 - 1.0,
            step: 1.0,
            group: "General",
            options,
            advanced: false,
        }
    }

    /// File this control under a named panel section.
    pub const fn group(mut self, group: &'static str) -> Self {
        self.group = group;
        self
    }

    /// File this control behind its group's disclosure.
    pub const fn advanced(mut self) -> Self {
        self.advanced = true;
        self
    }

    /// Set slider granularity.
    pub const fn step(mut self, step: f32) -> Self {
        self.step = step;
        self
    }

    /// The default encoded as the word that would be stored for it.
    pub fn default_word(&self) -> u32 {
        match self.kind {
            ParamKind::Float => self.default.to_bits(),
            _ => self.default as u32,
        }
    }
}

/// Declare an app's parameters.
///
/// Generates `SCHEMA` plus one `usize` constant per parameter, in declaration order:
///
/// ```ignore
/// fathom_core::params! {
///     G       => ParamDef::float("g", "Gravity", 1.0, 0.0, 5.0).group("Physics"),
///     TRAILS  => ParamDef::toggle("trails", "Trails", true).group("Render"),
/// }
/// // -> pub const SCHEMA: &[ParamDef]; pub const G: usize = 0; pub const TRAILS: usize = 1;
/// ```
#[macro_export]
macro_rules! params {
    ($($name:ident => $def:expr),* $(,)?) => {
        /// Declared parameters, in block order.
        pub const SCHEMA: &[$crate::params::ParamDef] = &[$($def),*];
        $crate::__params_index!(0usize; $($name,)*);
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __params_index {
    ($i:expr;) => {};
    ($i:expr; $head:ident, $($rest:ident,)*) => {
        #[allow(dead_code)]
        pub const $head: usize = $i;
        $crate::__params_index!($i + 1usize; $($rest,)*);
    };
}

/// The live values for a schema: one word per parameter, padded for uniform upload.
#[derive(Clone, Debug)]
pub struct ParamBlock {
    words: Vec<u32>,
    len: usize,
}

impl ParamBlock {
    /// A block holding every parameter's declared default.
    pub fn from_defaults(schema: &[ParamDef]) -> Self {
        let mut words: Vec<u32> = schema.iter().map(ParamDef::default_word).collect();
        let len = words.len();
        while words.len() % 4 != 0 || words.is_empty() {
            words.push(0);
        }
        Self { words, len }
    }

    /// Number of declared parameters (excluding padding).
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Padded size in bytes — what the UI mirror and the uniform buffer both use.
    pub fn byte_len(&self) -> usize {
        self.words.len() * 4
    }

    pub fn as_bytes(&self) -> &[u8] {
        bytemuck::cast_slice(&self.words)
    }

    pub fn words(&self) -> &[u32] {
        &self.words
    }

    /// Read a float parameter by index constant.
    pub fn f32(&self, index: usize) -> f32 {
        self.words.get(index).map_or(0.0, |w| f32::from_bits(*w))
    }

    /// Read an int, toggle or choice parameter by index constant.
    pub fn u32(&self, index: usize) -> u32 {
        self.words.get(index).copied().unwrap_or(0)
    }

    /// Write a float parameter by index constant.
    ///
    /// An in-process interface edits the block directly through these; the web and Tauri
    /// hosts instead ship a mirror of the whole block once a frame, because they are on
    /// the other side of a language boundary.
    pub fn set_f32(&mut self, index: usize, value: f32) {
        if let Some(word) = self.words.get_mut(index) {
            *word = value.to_bits();
        }
    }

    /// Write an int, toggle or choice parameter by index constant.
    pub fn set_u32(&mut self, index: usize, value: u32) {
        if let Some(word) = self.words.get_mut(index) {
            *word = value;
        }
    }

    /// Overwrite from a mirror sent by the UI. Short or long input is clamped rather
    /// than rejected, so a UI built against a stale schema degrades instead of dying.
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        let n = (bytes.len() / 4).min(self.words.len());
        for i in 0..n {
            let b = [bytes[i * 4], bytes[i * 4 + 1], bytes[i * 4 + 2], bytes[i * 4 + 3]];
            self.words[i] = u32::from_le_bytes(b);
        }
    }
}

/// A read-only view of the parameter values, handed to apps each frame.
#[derive(Clone, Copy)]
pub struct Params<'a> {
    schema: &'a [ParamDef],
    words: &'a [u32],
}

impl<'a> Params<'a> {
    pub fn new(schema: &'a [ParamDef], block: &'a ParamBlock) -> Self {
        Self { schema, words: block.words() }
    }

    pub fn schema(&self) -> &'a [ParamDef] {
        self.schema
    }

    /// Read a float parameter by index constant. Out-of-range indices read `0.0`.
    pub fn float(&self, index: usize) -> f32 {
        self.words.get(index).map_or(0.0, |w| f32::from_bits(*w))
    }

    /// Read an int or choice parameter by index constant.
    pub fn int(&self, index: usize) -> u32 {
        self.words.get(index).copied().unwrap_or(0)
    }

    /// Read a toggle by index constant.
    pub fn toggle(&self, index: usize) -> bool {
        self.int(index) != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMA: &[ParamDef] = &[
        ParamDef::float("g", "Gravity", 1.5, 0.0, 5.0),
        ParamDef::toggle("trails", "Trails", true),
        ParamDef::choice("mode", "Mode", 2, &["a", "b", "c"]),
    ];

    #[test]
    fn defaults_round_trip_through_the_block() {
        let block = ParamBlock::from_defaults(SCHEMA);
        let p = Params::new(SCHEMA, &block);
        assert_eq!(p.float(0), 1.5);
        assert!(p.toggle(1));
        assert_eq!(p.int(2), 2);
    }

    #[test]
    fn a_control_is_shown_with_the_rest_unless_it_says_otherwise() {
        let plain = ParamDef::float("a", "A", 0.0, 0.0, 1.0);
        assert!(!plain.advanced, "controls should default to being shown");
        assert!(plain.group("Physics").advanced().advanced);
        // Every kind can be filed away, not only sliders.
        assert!(ParamDef::int("b", "B", 1, 0, 4).advanced().advanced);
        assert!(ParamDef::toggle("c", "C", true).advanced().advanced);
        assert!(ParamDef::choice("d", "D", 0, &["x", "y"]).advanced().advanced);
    }

    #[test]
    fn block_is_padded_to_a_uniform_friendly_size() {
        let block = ParamBlock::from_defaults(SCHEMA);
        assert_eq!(block.len(), 3);
        assert_eq!(block.byte_len() % 16, 0);
        assert_eq!(block.byte_len(), 16);
    }

    #[test]
    fn writes_from_the_ui_mirror_land_at_the_right_offsets() {
        let mut block = ParamBlock::from_defaults(SCHEMA);
        let mut bytes = block.as_bytes().to_vec();
        bytes[0..4].copy_from_slice(&2.25f32.to_bits().to_le_bytes());
        bytes[4..8].copy_from_slice(&0u32.to_le_bytes());
        block.write_bytes(&bytes);

        let p = Params::new(SCHEMA, &block);
        assert_eq!(p.float(0), 2.25);
        assert!(!p.toggle(1));
        assert_eq!(p.int(2), 2, "untouched parameters keep their value");
    }

    #[test]
    fn a_short_write_leaves_the_remaining_parameters_alone() {
        let mut block = ParamBlock::from_defaults(SCHEMA);
        block.write_bytes(&0.5f32.to_bits().to_le_bytes());
        let p = Params::new(SCHEMA, &block);
        assert_eq!(p.float(0), 0.5);
        assert!(p.toggle(1));
    }

    #[test]
    fn values_can_be_edited_in_place_by_an_in_process_interface() {
        let mut block = ParamBlock::from_defaults(SCHEMA);
        block.set_f32(0, 3.25);
        block.set_u32(1, 0);
        assert_eq!(block.f32(0), 3.25);
        assert_eq!(block.u32(1), 0);

        let p = Params::new(SCHEMA, &block);
        assert_eq!(p.float(0), 3.25);
        assert!(!p.toggle(1));
    }

    #[test]
    fn writing_past_the_end_of_the_block_is_ignored() {
        let mut block = ParamBlock::from_defaults(SCHEMA);
        block.set_f32(99, 1.0);
        assert_eq!(block.f32(99), 0.0);
    }

    #[test]
    fn empty_schemas_still_produce_an_uploadable_block() {
        let block = ParamBlock::from_defaults(&[]);
        assert!(block.is_empty());
        assert_eq!(block.byte_len(), 16);
    }
}
