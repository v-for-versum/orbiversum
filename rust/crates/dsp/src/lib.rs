#![no_std]
#![cfg_attr(target_arch = "xtensa", feature(asm_experimental_arch))]
extern crate alloc;

pub mod prof;
pub mod sine;
pub mod starlids;
pub mod voice;
