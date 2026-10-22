// unwrap for results whose error type has no values

use core::convert::Infallible;

pub trait InfallibleExt<T> {
    /// The `Ok` value; compiles only while the error type is uninhabited, so a
    /// drawing target that gains a real error breaks the build instead of
    /// panicking on the device.
    fn unwrap_infallible(self) -> T;
}

impl<T> InfallibleExt<T> for Result<T, Infallible> {
    fn unwrap_infallible(self) -> T {
        match self {
            Ok(value) => value,
            Err(never) => match never {},
        }
    }
}
