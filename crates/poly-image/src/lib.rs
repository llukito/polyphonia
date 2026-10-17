//! Image carriers for Polyphonia (role 2).

pub mod damage;
mod load;
mod robust;
mod stealth;

pub use damage::{DamagedImage, ImageDamage, damage_image};
pub use robust::RobustPngCarrier;
pub use stealth::StealthPngCarrier;
