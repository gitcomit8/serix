#![no_std]
#![no_main]

use uefi::prelude::*;
use log::info;

#[entry]
fn main() -> Status {
    uefi::helpers::init().unwrap();
    info!("Hello, world!");
    boot::stall(5_000_000);
    Status::SUCCESS
}