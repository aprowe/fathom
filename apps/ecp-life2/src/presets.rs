//! Places worth starting from.
//!
//! A preset is not just a set of slider values. Which landscape you are standing on
//! matters at least as much as how hard you push on it — the same settings over
//! different terrain do something else entirely — so a preset carries its landscape seed
//! and its detail level too, and applying one rebakes the surface.
//!
//! They are compiled in rather than stored. There is nothing to migrate, nothing to
//! invalidate, and a preset that stops making sense because the parameter it referred to
//! changed meaning is a compile error rather than a puzzling simulation.

use fathom_core::CommandCtx;

use crate::scenes::Scene;
use crate::{
    AMP_CHASE, AMP_SYM, CHASE_GAIN, CORE_FRAC, DAMPING, HARMONICS, K_REP, LAM, PROFILE, R_CUT,
    RSTAR_MIN, RSTAR_SPAN, S0, SUBSTEPS, TIMESCALE, WELL_SIGMA,
};

pub struct Preset {
    pub name: &'static str,
    /// Which landscape. Two draws come off this seed — the interaction surface and the
    /// preferred-separation surface — so one number fixes the whole terrain.
    pub seed: u64,
    pub harmonics: u32,
    pub scene: Scene,
    pub floats: &'static [(usize, f32)],
    pub ints: &'static [(usize, u32)],
}

impl Preset {
    /// Move the panel to this preset. Parameters not mentioned are left alone: render
    /// and instrument settings are the viewer's, not the preset's, and resetting
    /// somebody's point size because they wanted a different physics would be rude.
    pub fn apply(&self, ctx: &mut CommandCtx<'_>) {
        for &(index, value) in self.floats {
            ctx.set_float(index, value);
        }
        for &(index, value) in self.ints {
            ctx.set_int(index, value);
        }
    }
}

pub const ALL: &[Preset] = &[
    // The shape the original CPU build validated: a monotonic profile, a modest chase and
    // heavy baseline attraction. Every pair wants the same separation, so this cannot
    // form much structure — but it binds reliably and conserves cleanly, which makes it
    // the one to come back to when something else has gone strange.
    Preset {
        name: "Reference pair",
        seed: 1,
        harmonics: 2,
        scene: Scene::Opposed,
        floats: &[
            (R_CUT, 0.024),
            (CORE_FRAC, 0.30),
            (K_REP, 1.5),
            (LAM, 0.6),
            (CHASE_GAIN, 1.0),
            (DAMPING, 400.0),
            (TIMESCALE, 0.15),
            (S0, 1.2),
            (AMP_SYM, 0.5),
            (AMP_CHASE, 0.35),
        ],
        ints: &[(PROFILE, 0), (HARMONICS, 2), (SUBSTEPS, 4)],
    },
    // The well profile doing what the classic one cannot: colour decides how far apart a
    // pair wants to sit, so particles sort themselves into shells at their own preferred
    // radii and the whole thing bands.
    Preset {
        name: "Bands",
        seed: 6,
        harmonics: 3,
        scene: Scene::Bands,
        floats: &[
            (R_CUT, 0.026),
            (CORE_FRAC, 0.25),
            (K_REP, 2.0),
            (LAM, 0.5),
            (CHASE_GAIN, 1.0),
            (DAMPING, 700.0),
            (TIMESCALE, 0.20),
            (S0, 0.8),
            (AMP_SYM, 0.9),
            (AMP_CHASE, 0.6),
            (WELL_SIGMA, 0.18),
            (RSTAR_MIN, 0.30),
            (RSTAR_SPAN, 0.55),
        ],
        ints: &[(PROFILE, 1), (HARMONICS, 3), (SUBSTEPS, 4)],
    },
    // A narrow well and a large colour store: colour becomes the slow variable, so groups
    // hold their composition long enough to have an identity while still burning.
    Preset {
        name: "Cells",
        seed: 12,
        harmonics: 4,
        scene: Scene::Clumps,
        floats: &[
            (R_CUT, 0.020),
            (CORE_FRAC, 0.35),
            (K_REP, 2.5),
            (LAM, 1.8),
            (CHASE_GAIN, 1.2),
            (DAMPING, 900.0),
            (TIMESCALE, 0.18),
            (S0, 0.7),
            (AMP_SYM, 1.1),
            (AMP_CHASE, 0.8),
            (WELL_SIGMA, 0.10),
            (RSTAR_MIN, 0.22),
            (RSTAR_SPAN, 0.30),
        ],
        ints: &[(PROFILE, 1), (HARMONICS, 4), (SUBSTEPS, 6)],
    },
    // Light damping and a wide, finely divided landscape: colours a little apart want
    // quite different distances, and the result strings out rather than balling up.
    Preset {
        name: "Filaments",
        seed: 23,
        harmonics: 5,
        scene: Scene::Soup,
        floats: &[
            (R_CUT, 0.030),
            (CORE_FRAC, 0.20),
            (K_REP, 1.2),
            (LAM, 0.35),
            (CHASE_GAIN, 1.4),
            (DAMPING, 250.0),
            (TIMESCALE, 0.12),
            (S0, 0.55),
            (AMP_SYM, 1.2),
            (AMP_CHASE, 1.0),
            (WELL_SIGMA, 0.22),
            (RSTAR_MIN, 0.35),
            (RSTAR_SPAN, 0.55),
        ],
        ints: &[(PROFILE, 1), (HARMONICS, 5), (SUBSTEPS, 8)],
    },
    // The chase switched off entirely. What is left is an ordinary conservative pair
    // potential with frozen colours: no engine, no ledger, just the landscape's own shape
    // settling out. Worth a look before deciding what the chase added.
    Preset {
        name: "Frozen landscape",
        seed: 6,
        harmonics: 3,
        scene: Scene::Soup,
        floats: &[
            (R_CUT, 0.026),
            (CORE_FRAC, 0.25),
            (K_REP, 2.0),
            (LAM, 0.5),
            (CHASE_GAIN, 0.0),
            (DAMPING, 800.0),
            (TIMESCALE, 0.25),
            (S0, 0.8),
            (AMP_SYM, 1.0),
            (AMP_CHASE, 0.7),
            (WELL_SIGMA, 0.18),
            (RSTAR_MIN, 0.30),
            (RSTAR_SPAN, 0.55),
        ],
        ints: &[(PROFILE, 1), (HARMONICS, 3), (SUBSTEPS, 4)],
    },
];

pub const LABELS: &[&str] =
    &["Reference pair", "Bands", "Cells", "Filaments", "Frozen landscape"];

pub fn from_index(i: u32) -> &'static Preset {
    &ALL[(i as usize).min(ALL.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SCHEMA, settings};
    use fathom_core::{ParamBlock, ParamKind, Params};

    /// A preset that sets a slider outside its own declared range leaves the panel
    /// showing a value it cannot get back to once it has been nudged.
    #[test]
    fn every_preset_stays_inside_the_ranges_it_writes_to() {
        for preset in ALL {
            for &(index, value) in preset.floats {
                let def = &SCHEMA[index];
                assert!(
                    value >= def.min && value <= def.max,
                    "{}: {} = {value} outside {}..{}",
                    preset.name,
                    def.name,
                    def.min,
                    def.max
                );
            }
            for &(index, value) in preset.ints {
                let def = &SCHEMA[index];
                let value = value as f32;
                assert!(
                    value >= def.min && value <= def.max,
                    "{}: {} = {value} outside {}..{}",
                    preset.name,
                    def.name,
                    def.min,
                    def.max
                );
            }
        }
    }

    #[test]
    fn presets_write_floats_to_float_controls_and_ints_to_the_rest() {
        for preset in ALL {
            for &(index, _) in preset.floats {
                assert!(
                    matches!(SCHEMA[index].kind, ParamKind::Float),
                    "{}: {} is not a slider",
                    preset.name,
                    SCHEMA[index].name
                );
            }
            for &(index, _) in preset.ints {
                assert!(
                    !matches!(SCHEMA[index].kind, ParamKind::Float),
                    "{}: {} is a slider",
                    preset.name,
                    SCHEMA[index].name
                );
            }
        }
    }

    /// The detail the preset bakes its landscape at and the detail its panel shows have
    /// to agree, or moving any other slider rebakes the terrain underneath the user.
    #[test]
    fn each_preset_declares_the_detail_its_panel_will_show() {
        for preset in ALL {
            let shown = preset.ints.iter().find(|(i, _)| *i == HARMONICS).map(|(_, v)| *v);
            assert_eq!(
                shown,
                Some(preset.harmonics),
                "{}: bakes at {} but shows {shown:?}",
                preset.name,
                preset.harmonics
            );
        }
    }

    #[test]
    fn every_preset_resolves_into_a_configuration_the_simulation_can_run() {
        for preset in ALL {
            let mut block = ParamBlock::from_defaults(SCHEMA);
            for &(i, v) in preset.floats {
                block.set_f32(i, v);
            }
            for &(i, v) in preset.ints {
                block.set_u32(i, v);
            }
            let s = settings(Params::new(SCHEMA, &block));
            assert!(s.r_core < s.r_cut, "{}: core fills the interaction range", preset.name);
            assert!(
                s.rstar_min + s.rstar_span <= s.r_cut,
                "{}: well sits outside the cutoff",
                preset.name
            );
            assert!(1.0 / s.cells() as f32 >= s.r_cut, "{}: cutoff outruns its cell", preset.name);
        }
    }

    #[test]
    fn the_labels_match_the_presets_they_name() {
        assert_eq!(LABELS.len(), ALL.len());
        for (preset, label) in ALL.iter().zip(LABELS) {
            assert_eq!(&preset.name, label);
        }
    }

    /// The frozen preset's whole point is that the engine is off. If it ever acquires a
    /// chase it stops being the thing you compare the others against.
    #[test]
    fn the_frozen_preset_has_no_chase() {
        let frozen = ALL.iter().find(|p| p.name == "Frozen landscape").expect("preset missing");
        let gain = frozen.floats.iter().find(|(i, _)| *i == CHASE_GAIN).map(|(_, v)| *v);
        assert_eq!(gain, Some(0.0));
    }
}
