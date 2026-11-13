use orbiversum_core::prof;

pub const P_CONTROL: usize = 0;
pub const P_PAD: usize = 1;
pub const P_MOOG: usize = 2;
pub const P_KLANK: usize = 3;
pub const P_PIANO: usize = 4;
pub const P_PERC: usize = 5;
pub const P_REVERB: usize = 6;
pub const P_FINAL: usize = 7;
/// Profile section names, same order as the `P_*` consts.
pub static ENO_NAMES: [&str; prof::SECTIONS] = ["control", "pad", "moog", "klank", "piano", "perc", "reverb", "final"];
