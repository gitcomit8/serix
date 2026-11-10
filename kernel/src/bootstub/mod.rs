/*
 * bootstub/mod.rs - Conditional bootstub selection
 *
 * Selects which bootloader stub to compile based on Cargo features.
 * Only one feature may be enabled at a time.
 */

#[cfg(feature = "bootstub-limine")]
pub mod limine;

#[cfg(feature = "bootstub-grub2")]
pub mod grub2;

#[cfg(feature = "bootstub-edk2")]
pub mod edk2;

/* Re-export serial_println for bootstub modules */
pub use hal::serial_println;

/* Ensure exactly one bootstub feature is enabled */
#[cfg(not(any(
    feature = "bootstub-limine",
    feature = "bootstub-grub2",
    feature = "bootstub-edk2",
)))]
compile_error!("Exactly one bootstub feature must be enabled: bootstub-limine, bootstub-grub2, or bootstub-edk2");

#[cfg(all(
    any(feature = "bootstub-limine", feature = "bootstub-grub2"),
    feature = "bootstub-edk2",
))]
compile_error!("bootstub-edk2 cannot be combined with other bootstub features");
