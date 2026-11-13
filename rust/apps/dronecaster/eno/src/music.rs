//! Musical material: note rates, sequences, chord planes, string tuning.

/// Note-rate choices ([0.02,0.05,1,2,0.5,0.25,2] / 2).
pub(crate) const NOTE_RATES: [f32; 7] = [0.01, 0.025, 0.5, 1.0, 0.25, 0.125, 1.0];
pub(crate) const AIRPORTS: &[&[i8]] = &[
    &[5, 7, 4, 2, 0, 12, 7, 5, 7, 4, 2, 0],
    &[5, 7, 4, 2, 0, 12, 4, 7, 5, 0],
    &[-5, 2, 0, 4, 7, 12, 5, 2, 7, 4, 0, 7, 2, 5, 5, 2, 4, 0],
    &[7, 7, 2, 4, 4, 4, 2, 0, 7, 0, 0],
];
pub(crate) const TUNE_UP: f32 = 1.0005;
pub(crate) const TUNE_DOWN: f32 = 0.9996;
pub(crate) const STRING_DECAY: f32 = 6.0;

pub(crate) const PLANES: [[i8; 4]; 4] = [[0, 4, 7, 12], [4, 7, 11, 16], [-3, 0, 4, 7], [-3, 0, 5, 9]];
