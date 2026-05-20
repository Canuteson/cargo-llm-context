pub mod owned;
pub mod shared;
pub mod handles;
pub mod borrowed;

// Internal module — items surfaced only via pub use below.
mod internal;
pub use internal::Forwarded;
